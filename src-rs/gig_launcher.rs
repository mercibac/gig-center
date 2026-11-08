use crate::settings::Settings;
use arboard::Clipboard;
use enigo::{Button, Coordinate, Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings};
use std::os::windows::process::CommandExt;
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

fn load_the_most_recent_file(enigo: &mut Enigo) {
    let _ = enigo.move_mouse(14, 62, Coordinate::Abs);
    let _ = enigo.button(Button::Left, enigo::Direction::Click);
    thread::sleep(Duration::from_millis(500));
    let _ = enigo.move_mouse(99, 100, Coordinate::Abs);
    thread::sleep(Duration::from_millis(500));
    let _ = enigo.move_mouse(275, 189, Coordinate::Abs);
    thread::sleep(Duration::from_millis(500));
    let _ = enigo.move_mouse(450, 189, Coordinate::Abs);
    let _ = enigo.button(Button::Left, enigo::Direction::Click);
}

fn launch_programs(settings: &Settings) {
    let gig_performer_path = r"C:\Program Files\Deskew Technologies\Gig Performer 5\GigPerformer5.exe";
    let osc_path = r"D:\Apps\portable\open-stage-control_1.30.1_win32-x64\open-stage-control.exe";

    console_log!("Launching Gig Performer...");
    let _ = Command::new("cmd")
        .args(["/C", "start", "", gig_performer_path])
        .spawn();

    console_log!("Launching Open Stage Control...");
    let _ = Command::new("cmd")
        .args(["/C", "start", "", osc_path])
        .spawn();

    thread::sleep(Duration::from_secs(12));

    let ip_address = get_wifi_ip().unwrap_or_else(|| {
        console_log!("Could not find Wi-Fi IP address. Defaulting to localhost.");
        "127.0.0.1".to_string()
    });

    let send_value = format!("{}:54344", ip_address);
    if let Ok(mut clipboard) = Clipboard::new() {
        let _ = clipboard.set_text(&send_value);
        console_log!("Copied {} to clipboard.", send_value);
    }

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

    // Losing the input device only costs us the Open Stage Control setup — the
    // phone still gets the URL below, so log it and carry on rather than panic.
    match Enigo::new(&EnigoSettings::default()) {
        Ok(mut enigo) => {
            let _ = enigo.move_mouse(300, 110, Coordinate::Abs);
            let _ = enigo.button(Button::Left, enigo::Direction::Click);
            thread::sleep(Duration::from_millis(500));

            // Ctrl+A, Ctrl+V
            let _ = enigo.key(Key::Control, enigo::Direction::Press);
            let _ = enigo.key(Key::Unicode('a'), enigo::Direction::Click);
            let _ = enigo.key(Key::Unicode('v'), enigo::Direction::Click);
            let _ = enigo.key(Key::Control, enigo::Direction::Release);

            thread::sleep(Duration::from_secs(1));

            // Click play
            let _ = enigo.move_mouse(62, 59, Coordinate::Abs);
            let _ = enigo.button(Button::Left, enigo::Direction::Click);

            thread::sleep(Duration::from_secs(3));

            let osc_file = crate::utils::resource_path(&settings.osc_file_path);
            if !osc_file.exists() || settings.osc_file_path.is_empty() {
                load_the_most_recent_file(&mut enigo);
            }
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
