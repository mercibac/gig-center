use std::process::Command;
use std::thread;
use std::time::Duration;

/// Asks Windows for the name of the currently connected Wi-Fi network.
pub fn get_current_wifi() -> Option<String> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // We want the "SSID" line, but not the "BSSID" (MAC address) line
        if line.contains("SSID") && !line.contains("BSSID") {
            if let Some(value) = line.split(':').nth(1) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Forces Windows to connect to a known Wi-Fi profile.
pub fn connect_to_network(ssid_name: &str) {
    console_log!("Attempting to connect to '{}'...", ssid_name);

    let result = Command::new("netsh")
        .args(["wlan", "connect", &format!("name={}", ssid_name)])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            // Give Windows time to negotiate the IP address with the phone
            console_log!("Waiting for connection to stabilize...");
            thread::sleep(Duration::from_secs(30));
        }
        _ => {
            console_log!(
                "Failed to connect. Make sure the profile for '{}' is saved on this PC.",
                ssid_name
            );
        }
    }
}

/// The main logic to check and switch networks.
pub fn ensure_correct_network(target_ssid: &str) -> bool {
    console_log!("Checking network connection...");
    let current_network = get_current_wifi();

    if current_network.as_deref() == Some(target_ssid) {
        console_log!("Success: PC is already connected to '{}'.", target_ssid);
        return true;
    }

    match &current_network {
        Some(name) => console_log!("Warning: Connected to the wrong network ('{}').", name),
        None => console_log!("Warning: Not connected to any Wi-Fi network."),
    }

    connect_to_network(target_ssid);

    // Verify the connection worked
    if get_current_wifi().as_deref() == Some(target_ssid) {
        console_log!("Success: Successfully switched to '{}'.", target_ssid);
        true
    } else {
        console_log!(
            "Error: Could not connect to '{}'. Halting gig setup.",
            target_ssid
        );
        false
    }
}
