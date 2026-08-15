use std::process::Command;
use std::thread;
use std::time::Duration;

/// Toggle Windows sleep and monitor timeout settings via powercfg.
/// `option` = "on"  → restore 10-minute timeouts
/// `option` = "off" → disable timeouts (set to 0)
pub fn switch_sleep(option: &str) {
    let minutes = match option {
        "on" => "10",
        "off" => "0",
        _ => return,
    };

    let settings = [
        "standby-timeout-ac",
        "monitor-timeout-ac",
        "standby-timeout-dc",
        "monitor-timeout-dc",
    ];

    for setting in &settings {
        let result = Command::new("powercfg")
            .args(["/change", setting, minutes])
            .output();

        if let Err(e) = result {
            console_log!("Failed to change sleep settings: {}", e);
            return;
        }
    }

    console_log!("Screen settings has been changed successfully");
    thread::sleep(Duration::from_secs(1));
}
