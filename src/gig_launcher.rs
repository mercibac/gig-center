use crate::settings::Settings;
use enigo::{Enigo, Key, Keyboard, Settings as EnigoSettings};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};

fn get_wifi_ip() -> Option<String> {
    // Keep ipconfig's console host hidden so no window flashes on screen.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("ipconfig")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut in_wifi = false;

    for line in text.lines() {
        let line = line.trim();
        let lower = line.to_lowercase();
        if lower.starts_with("wireless") || lower.starts_with("wi-fi") {
            in_wifi = true;
        } else if lower.starts_with("ethernet adapter") || lower.starts_with("unknown adapter") {
            in_wifi = false;
        }

        if in_wifi && lower.starts_with("ipv4 address") {
            if let Some(ip) = line.split(':').nth(1) {
                return Some(ip.trim().to_string());
            }
        }
    }
    None
}

fn run_cmd(args: &[&str]) {
    let timeout = Duration::from_secs(2);

    // KDE Connect uses Qt, which needs a valid window handle for its event loop.
    // CREATE_NO_WINDOW kills the handle entirely and breaks Qt's PostMessage.
    // Instead, we create a new (hidden) console so Qt works but the user sees nothing.
    // This matches Python's STARTUPINFO(dwFlags=STARTF_USESHOWWINDOW, wShowWindow=SW_HIDE).
    const CREATE_NEW_CONSOLE: u32 = 0x00000010;

    let child = Command::new(args[0])
        .args(&args[1..])
        .creation_flags(CREATE_NEW_CONSOLE)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process finished — read output
                        let mut stderr_buf = Vec::new();
                        if let Some(mut stderr) = child.stderr.take() {
                            use std::io::Read;
                            let _ = stderr.read_to_end(&mut stderr_buf);
                        }
                        if status.success() {
                            console_log!("{} runs successfuly!", args[0]);
                        } else {
                            console_log!(
                                "Attempt {} failed: Command returned non-zero exit status.",
                                args[0]
                            );
                            console_log!(
                                "Error output: {}",
                                String::from_utf8_lossy(&stderr_buf).trim()
                            );
                        }
                        break;
                    }
                    Ok(None) => {
                        // Still running — check timeout
                        if start.elapsed() >= timeout {
                            console_log!(
                                "Attempt {} failed: Command timed out after {}s.",
                                args[0],
                                timeout.as_secs()
                            );
                            let _ = child.kill();
                            let _ = child.wait(); // Reap the zombie
                            break;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        console_log!("An unexpected error occurred: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => console_log!("An unexpected error occurred: {}", e),
    }
}

fn open_url_on_phone(url: &str, phone_name: &str) {
    console_log!("Sending {} to {}...", url, phone_name);

    run_cmd(&["kdeconnect-cli", "--refresh"]);
    thread::sleep(Duration::from_secs(7));
    run_cmd(&["kdeconnect-cli", "--ping", "--name", phone_name]);
    thread::sleep(Duration::from_secs(2));
    run_cmd(&["kdeconnect-cli", "--share", url, "--name", phone_name]);
}

/// Where Gig Performer puts itself.
const GIG_PERFORMER_DEFAULT: &str =
    r"C:\Program Files\Deskew Technologies\Gig Performer 5\GigPerformer5.exe";

/// Where Open Stage Control lives on this rig. The folder name carries the
/// version, so this one goes stale on every upgrade — which is the reason the
/// Advanced section can override it.
const OPEN_STAGE_CONTROL_DEFAULT: &str =
    r"D:\Apps\portable\open-stage-control_1.30.1_win32-x64\open-stage-control.exe";

/// The executable to start: the one chosen in the Advanced settings, or the
/// default above when that field is empty or names something that is not there.
///
/// Falling back rather than failing keeps LAUNCH behaving exactly as it did
/// before the field existed, so a path left over from an uninstalled version
/// costs a console line instead of the gig rig.
fn executable<'a>(chosen: &'a str, default: &'a str, name: &str) -> &'a str {
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return default;
    }
    if Path::new(chosen).is_file() {
        return chosen;
    }
    console_log!("The {name} path set in Settings is not there: {chosen}");
    console_log!("Falling back to {default}");
    default
}

fn launch_programs(settings: &Settings) {
    let gig_performer_path = executable(
        &settings.executables.gig_performer,
        GIG_PERFORMER_DEFAULT,
        "Gig Performer",
    );
    let osc_path = executable(
        &settings.executables.open_stage_control,
        OPEN_STAGE_CONTROL_DEFAULT,
        "Open Stage Control",
    );

    // The path is logged because it is now a setting: when the wrong build
    // starts, the console says which one was asked for.
    console_log!("Launching Gig Performer... ({gig_performer_path})");
    let _ = Command::new("cmd")
        .args(["/C", "start", "", gig_performer_path])
        .spawn();

    console_log!("Launching Open Stage Control... ({osc_path})");
    let _ = Command::new("cmd")
        .args(["/C", "start", "", osc_path])
        .spawn();

    thread::sleep(Duration::from_secs(12));

    let ip_address = get_wifi_ip().unwrap_or_else(|| {
        console_log!("Could not find Wi-Fi IP address. Defaulting to localhost.");
        "127.0.0.1".to_string()
    });

    // Bring OSC to top
    if let Some(hwnd) = crate::snap::find_window_by_title("Open Stage Control") {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        thread::sleep(Duration::from_secs(1));
    } else {
        console_log!("Could not find the Open Stage Control window to foreground.");
    }

    // phone still gets the URL below, so log it and carry on rather than panic.
    match Enigo::new(&EnigoSettings::default()) {
        Ok(mut enigo) => {
            thread::sleep(Duration::from_secs(2));

            //Start the server
            let _ = enigo.key(Key::F5, enigo::Direction::Click);
            thread::sleep(Duration::from_secs(3));

        }
        Err(e) => {
            console_log!("Error: could not take control of the keyboard/mouse ({e}).");
            console_log!("Open Stage Control was launched but not configured — set it up by hand.");
        }
    }

    open_url_on_phone(&format!("http://{}:8080", ip_address), &settings.phone_name);
}

pub fn main(settings: Settings) {
    crate::utils::time_it("gig_launcher::main", || {
        let target = &settings.hotspot_name;

        crate::switch_sleep::switch_sleep("off");
        if settings.manage_defender {
            crate::switch_defender::switch_defender("off");
        } else {
            console_log!("Skipping Windows Defender — the Defender button is off.");
        }
        crate::utils::set_volume(90);

        let network_ready = crate::connection::ensure_correct_network(target);

        if network_ready {
            console_log!(
                "\nNetwork is secure. Ready to launch Gig Performer and Open Stage Control..."
            );
            launch_programs(&settings);
            thread::sleep(Duration::from_secs(10));
            crate::snap::snap_gig_windows("GigPerformer-PA-FULL");
            thread::sleep(Duration::from_secs(20));
            crate::snap::snap_gig_windows("Keyboard_Live");
        } else {
            console_log!("\nPlease check your phone hotspot and try again.");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A scratch path of its own per test. There is no `tempfile`
    /// dev-dependency and these tests do not need one.
    fn scratch(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("gig-center-{tag}-{unique}.exe"))
    }

    #[test]
    fn an_empty_field_uses_the_install_path() {
        assert_eq!(
            executable("", GIG_PERFORMER_DEFAULT, "Gig Performer"),
            GIG_PERFORMER_DEFAULT
        );
    }

    /// Whitespace is not a choice, so a field left holding a stray space still
    /// resolves to the install path rather than trying to run " ".
    #[test]
    fn a_blank_field_uses_the_install_path() {
        assert_eq!(
            executable("   ", OPEN_STAGE_CONTROL_DEFAULT, "Open Stage Control"),
            OPEN_STAGE_CONTROL_DEFAULT
        );
    }

    #[test]
    fn a_chosen_executable_that_is_there_is_used() {
        let path = scratch("chosen");
        fs::write(&path, b"not really an exe").expect("could not write the test file");
        let chosen = path.to_string_lossy().to_string();

        assert_eq!(
            executable(&chosen, GIG_PERFORMER_DEFAULT, "Gig Performer"),
            chosen
        );
        let _ = fs::remove_file(&path);
    }

    /// The case this guards: Open Stage Control is upgraded, the old versioned
    /// folder goes, and the saved path now points at nothing. LAUNCH has to
    /// carry on with the built-in path rather than start nothing at all.
    #[test]
    fn a_chosen_executable_that_is_gone_falls_back() {
        let missing = scratch("missing").to_string_lossy().to_string();
        assert_eq!(
            executable(&missing, OPEN_STAGE_CONTROL_DEFAULT, "Open Stage Control"),
            OPEN_STAGE_CONTROL_DEFAULT
        );
    }

    /// A directory is not something `start` can run, so it counts as missing.
    #[test]
    fn a_folder_is_not_an_executable() {
        let dir = std::env::temp_dir();
        let chosen = dir.to_string_lossy().to_string();
        assert_eq!(
            executable(&chosen, GIG_PERFORMER_DEFAULT, "Gig Performer"),
            GIG_PERFORMER_DEFAULT
        );
    }
}
