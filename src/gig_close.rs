use std::os::windows::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::System;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsWindowVisible, SendMessageW, WM_CLOSE,
};

/// Prevents a console window flashing up behind the shutdown command.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Gap between the last teardown step and the machine going down.
const SHUTDOWN_DELAY: Duration = Duration::from_secs(2);

/// Gig Performer, without its major version.
///
/// Version-agnostic on purpose. This read `GigPerformer4.exe` while LAUNCH
/// started `GigPerformer5.exe`, and because the match below is a `contains`,
/// the two never met: CLEAR DECK reported success having killed nothing, and
/// left Gig Performer holding the audio device. Pinning `5` instead would only
/// have moved that failure to the next major version, and the number buys
/// nothing a substring match needs.
const GIG_PERFORMER_PROCESS: &str = "GigPerformer";

/// Open Stage Control. Its executable carries no version.
const OPEN_STAGE_CONTROL_PROCESS: &str = "open-stage-control.exe";

/// Whether a running process is one of the ones to kill.
///
/// Pulled out of the loop below so the rule is testable without a live process
/// list, which is what lets the tests pin that Gig Performer still matches
/// across major versions. A case-insensitive substring, so a target may be a
/// whole executable name or just the part of it that does not change.
fn process_matches(process_name: &str, targets: &[&str]) -> bool {
    let process_name = process_name.to_lowercase();
    targets
        .iter()
        .any(|&t| process_name.contains(&t.to_lowercase()))
}

pub fn force_kill(target_processes: &[&str]) {
    console_log!("--- Phase 1: Force Killing Gig Apps ---");
    let mut sys = System::new_all();
    sys.refresh_processes();

    for process in sys.processes().values() {
        if process_matches(process.name(), target_processes) {
            console_log!("Force killing process: {}", process.name());
            process.kill();
        }
    }

    // Give Windows a second to free up the network ports and audio drivers
    thread::sleep(Duration::from_secs(1));
}

pub fn clear_the_deck(protected_titles: &[&str]) {
    console_log!("\n--- Phase 2: Politely Closing Other Windows ---");

    struct SearchData {
        protected: Vec<String>,
    }

    let mut data = SearchData {
        protected: protected_titles.iter().map(|s| s.to_lowercase()).collect(),
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &*(lparam.0 as *const SearchData);

        if IsWindowVisible(hwnd).as_bool() {
            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&title_buf[..len as usize]).to_lowercase();

                let is_program_manager = title.contains("program manager");
                let is_protected = data.protected.iter().any(|p| title.contains(p));

                if !is_program_manager && !is_protected {
                    console_log!("Politely closing window: {}", title);
                    let _ = SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
        BOOL(1) // Continue enumeration
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut data as *mut SearchData as isize),
        );
    }
}

/// Power the machine off, after a short pause.
///
/// The pause sits between the last teardown step and the shutdown so the
/// windows closing above have a moment to finish rather than being cut off.
pub fn shutdown_pc(delay: Duration) {
    console_log!("Shutting down the PC in {} seconds...", delay.as_secs());
    thread::sleep(delay);

    if let Err(e) = Command::new("shutdown")
        .args(["/s", "/t", "0"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
    {
        console_log!("Could not shut down the PC: {}", e);
    }
}

pub fn main(shutdown_after: bool, manage_defender: bool) {
    crate::switch_sleep::switch_sleep("on");
    if manage_defender {
        crate::switch_defender::switch_defender("on");
    } else {
        console_log!("Skipping Windows Defender — the Defender button is off.");
    }
    crate::utils::set_volume(50);

    let apps_to_nuke = [GIG_PERFORMER_PROCESS, OPEN_STAGE_CONTROL_PROCESS];
    let apps_to_protect = ["firefox", "gig control center", "wezterm"];

    force_kill(&apps_to_nuke);
    clear_the_deck(&apps_to_protect);

    console_log!("\nDone! Gig apps are completely terminated, background windows are closed, and Firefox is safe.");

    // Last thing, once every step above has finished.
    if shutdown_after {
        shutdown_pc(SHUTDOWN_DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real targets, not a copy of them, so these fail if the list drifts.
    const TARGETS: [&str; 2] = [GIG_PERFORMER_PROCESS, OPEN_STAGE_CONTROL_PROCESS];

    /// The bug this guards: the target read `GigPerformer4.exe` while LAUNCH
    /// started `GigPerformer5.exe`, so Phase 1 matched nothing and CLEAR DECK
    /// said it was done with Gig Performer still running.
    #[test]
    fn gig_performer_is_killed_whatever_its_major_version() {
        for name in [
            "GigPerformer4.exe",
            "GigPerformer5.exe",
            "GigPerformer6.exe",
        ] {
            assert!(
                process_matches(name, &TARGETS),
                "{name} would have been left running"
            );
        }
    }

    #[test]
    fn open_stage_control_is_killed() {
        assert!(process_matches("open-stage-control.exe", &TARGETS));
    }

    /// Phase 1 kills without asking, so the match must not reach anything that
    /// merely sits nearby — Gig Center itself above all, since killing it would
    /// take Phase 2 down with it.
    #[test]
    fn unrelated_processes_are_left_alone() {
        for name in [
            "firefox.exe",
            "explorer.exe",
            "gigcontrolcenter.exe",
            "gig-center.exe",
            "wezterm-gui.exe",
        ] {
            assert!(!process_matches(name, &TARGETS), "{name} would be killed");
        }
    }

    #[test]
    fn the_match_ignores_case() {
        assert!(process_matches("GIGPERFORMER5.EXE", &TARGETS));
        assert!(process_matches("OPEN-STAGE-CONTROL.EXE", &TARGETS));
    }
}
