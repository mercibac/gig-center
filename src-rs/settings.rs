use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub phone_name: String,
    pub hotspot_name: String,
    pub osc_file_path: String,
    /// MIDI input to watch for the activity LED. Empty until one is chosen.
    #[serde(default)]
    pub midi_device: String,
    /// Audio output to watch for the signal LED. Empty until one is chosen.
    #[serde(default)]
    pub audio_device: String,
    /// Power the PC off once CLEAR DECK has finished.
    #[serde(default)]
    pub shutdown_after_clear: bool,
    /// Whether LAUNCH and CLEAR DECK toggle Windows Defender at all.
    /// Defaults to true so an existing settings.json, written before this
    /// existed, keeps behaving as it always did.
    #[serde(default = "enabled")]
    pub manage_defender: bool,
}

/// serde needs a function for a non-false default.
fn enabled() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            phone_name: "SMART PHONE".to_string(),
            hotspot_name: "SHARED ACCESS POINT".to_string(),
            osc_file_path: String::new(),
            midi_device: String::new(),
            audio_device: String::new(),
            shutdown_after_clear: false,
            manage_defender: true,
        }
    }
}

/// Get the directory where settings.json is stored.
/// In release (compiled exe): next to the executable.
/// In development: current working directory.
fn config_dir() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
        if exe_dir.ends_with("debug") || exe_dir.ends_with("release") {
            return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        }
        return exe_dir.to_path_buf();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}


/// Get the full path to settings.json.
pub fn config_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Load settings from disk (always reads fresh).
/// Creates a default file if missing or corrupted.
pub fn load_settings() -> Settings {
    let path = config_path();
    if !path.exists() {
        let defaults = Settings::default();
        save_settings(&defaults);
        return defaults;
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("Warning: settings.json is corrupted. Repairing... ({e})");
            let defaults = Settings::default();
            save_settings(&defaults);
            defaults
        }),
        Err(e) => {
            eprintln!("Warning: Could not read settings.json. Repairing... ({e})");
            let defaults = Settings::default();
            save_settings(&defaults);
            defaults
        }
    }
}

/// Save settings to disk as pretty-printed JSON.
pub fn save_settings(settings: &Settings) {
    let path = config_path();
    let json = serde_json::to_string_pretty(settings).unwrap_or_default();
    if let Err(e) = fs::write(&path, json) {
        eprintln!("Error: Could not save settings.json: {e}");
    }
}
