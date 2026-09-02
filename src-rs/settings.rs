use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub phone_name: String,
    pub hotspot_name: String,
    pub osc_file_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            phone_name: "SMART PHONE".to_string(),
            hotspot_name: "SHARED ACCESS POINT".to_string(),
            osc_file_path: String::new(),
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
