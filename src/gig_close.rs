use std::thread;
use std::time::Duration;
use sysinfo::System;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsWindowVisible, SendMessageW, WM_CLOSE,
};

pub fn force_kill(target_processes: &[&str]) {
    console_log!("--- Phase 1: Force Killing Gig Apps ---");
    let mut sys = System::new_all();
    sys.refresh_processes();

    for (_pid, process) in sys.processes() {
        let process_name = process.name().to_lowercase();
        if target_processes
            .iter()
            .any(|&t| process_name.contains(&t.to_lowercase()))
        {
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

pub fn main() {
    crate::switch_sleep::switch_sleep("on");
    crate::switch_defender::switch_defender("on");
    crate::utils::set_volume(50);

    let apps_to_nuke = ["GigPerformer4.exe", "open-stage-control.exe"];
    let apps_to_protect = ["firefox", "gig control center", "wezterm"];

    force_kill(&apps_to_nuke);
    clear_the_deck(&apps_to_protect);

    console_log!("\nDone! Gig apps are completely terminated, background windows are closed, and Firefox is safe.");
}
