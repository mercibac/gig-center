use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Replacements for the screen-match images the Defender automation clicks.
///
/// The built-in ones are screenshots of one particular Defender UI, so a
/// different Windows build, language or display scaling needs different ones.
/// An empty entry means "use the image compiled into the exe" — which is both
/// the default and the reset, so clearing a field in the Advanced section
/// restores the original with nothing to undo.
///
/// Note that empty means something different here than it does for the device
/// fields in [`Settings`]; [`merge_from_form`] says why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefenderImages {
    /// The "Turn on" button on the "Virus & threat protection" card. Clicked
    /// during CLEAR DECK, and that one click is the whole of the way back on:
    /// the button switches real-time protection on by itself.
    #[serde(default)]
    pub turn_on: String,
    /// The "Virus & threat protection" card as it looks while real-time
    /// protection is *on*. Named for the state it shows rather than the step
    /// it is used in, which reads oddly until you see why: the card is matched
    /// *before* it is clicked, so the way *off* — during LAUNCH — is what has
    /// to look for the card as it appears while protection is still on.
    #[serde(default)]
    pub activation_state: String,
    /// The "Real-time protection" label the last click is offset from.
    #[serde(default)]
    pub realtime_label: String,
    /// The "Manage settings" link.
    #[serde(default)]
    pub manage_settings: String,
}

impl DefenderImages {
    /// Trim every path, the way the text fields are trimmed on save.
    fn trimmed(self) -> Self {
        Self {
            turn_on: self.turn_on.trim().to_string(),
            activation_state: self.activation_state.trim().to_string(),
            realtime_label: self.realtime_label.trim().to_string(),
            manage_settings: self.manage_settings.trim().to_string(),
        }
    }
}

/// Where the programs LAUNCH starts actually live.
///
/// An empty entry means the default install path compiled into the exe, the
/// same rule the image fields follow, so clearing a field is the way back to
/// the original. Worth having because the Open Stage Control default carries a
/// version number in its folder name, which made upgrading it a source edit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Executables {
    /// Gig Performer. Empty uses the default install path.
    #[serde(default)]
    pub gig_performer: String,
    /// Open Stage Control. Empty uses the default install path.
    #[serde(default)]
    pub open_stage_control: String,
}

impl Executables {
    /// Trim both paths, the way the text fields are trimmed on save.
    fn trimmed(self) -> Self {
        Self {
            gig_performer: self.gig_performer.trim().to_string(),
            open_stage_control: self.open_stage_control.trim().to_string(),
        }
    }
}

/// The window titles LAUNCH looks for when it snaps the two programs into their
/// halves of the screen.
///
/// Not paths, unlike every other group here: each is matched as a
/// case-insensitive part of a window's title bar. The Gig Performer one is
/// therefore usually the gig file's name, which is what Gig Performer puts up
/// there once the file has loaded — so renaming a gig file used to mean editing
/// `snap.rs` and rebuilding.
///
/// An empty entry means the name compiled into the exe, the same rule the image
/// and executable fields follow, so clearing a field is the way back to the
/// original. An empty one must never reach the matcher, because `contains("")`
/// is true of every window; [`crate::snap::window_name`] is the guard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowNames {
    /// The Open Stage Control window. Empty uses the built-in name.
    #[serde(default)]
    pub open_stage_control: String,
    /// The Gig Performer window. Empty uses the built-in name.
    #[serde(default)]
    pub gig_performer: String,
}

impl WindowNames {
    /// Trim both names, the way the text fields are trimmed on save.
    fn trimmed(self) -> Self {
        Self {
            open_stage_control: self.open_stage_control.trim().to_string(),
            gig_performer: self.gig_performer.trim().to_string(),
        }
    }
}

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
    /// Chosen replacements for the Defender screen-match images. All empty by
    /// default, so a settings.json written before this existed reads as the
    /// built-in images.
    #[serde(default)]
    pub defender_images: DefenderImages,
    /// Chosen locations for the programs LAUNCH starts. Both empty by default,
    /// so a settings.json written before this existed reads as the built-in
    /// install paths.
    #[serde(default)]
    pub executables: Executables,
    /// Chosen window titles for the snap step. Both empty by default, so a
    /// settings.json written before this existed reads as the built-in names.
    #[serde(default)]
    pub window_names: WindowNames,
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
            defender_images: DefenderImages::default(),
            executables: Executables::default(),
            window_names: WindowNames::default(),
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

/// Build the settings to save from the modal's fields, what is already on disk,
/// and the two dropdowns.
///
/// Lives here rather than inside the save callback so the rules are testable
/// without driving the UI. Three of them matter:
///
/// - The shutdown and Defender toggles live on the main screen, not this form,
///   so they are carried over from disk untouched.
/// - A dropdown reads empty when its device is not connected. Taking that at
///   face value would wipe the remembered device, so an empty one keeps
///   whatever was stored.
/// - The Advanced groups — the Defender images, the two executables and the two
///   window names — follow the opposite rule to the dropdowns: an empty one
///   means "use the built-in default", not "keep what was stored". Carrying the
///   stored value over instead would make the Default button impossible, since
///   clearing a field would just restore the old one on save.
pub fn merge_from_form(stored: Settings, form: FormInput) -> Settings {
    Settings {
        phone_name: form.phone.trim().to_string(),
        hotspot_name: form.hotspot.trim().to_string(),
        osc_file_path: form.osc_path.trim().to_string(),
        midi_device: if form.midi_device.is_empty() {
            stored.midi_device
        } else {
            form.midi_device.to_string()
        },
        audio_device: if form.audio_device.is_empty() {
            stored.audio_device
        } else {
            form.audio_device.to_string()
        },
        shutdown_after_clear: stored.shutdown_after_clear,
        manage_defender: stored.manage_defender,
        defender_images: form.defender_images.trimmed(),
        executables: form.executables.trimmed(),
        window_names: form.window_names.trimmed(),
    }
}

/// Everything the settings modal holds when SAVE is pressed.
///
/// A struct rather than a widening parameter list: the Advanced groups had
/// taken [`merge_from_form`] to eight positional arguments, four of them
/// same-typed strings sitting next to each other, where transposing two would
/// have compiled and quietly saved the hotspot name as the phone name. Naming
/// them also keeps the next group of options from growing the signature again.
pub struct FormInput<'a> {
    pub phone: &'a str,
    pub hotspot: &'a str,
    pub osc_path: &'a str,
    pub midi_device: &'a str,
    pub audio_device: &'a str,
    pub defender_images: DefenderImages,
    pub executables: Executables,
    pub window_names: WindowNames,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Settings {
        Settings {
            phone_name: "old".into(),
            hotspot_name: "old".into(),
            osc_file_path: "old.json".into(),
            midi_device: "Portable G-1 0".into(),
            audio_device: "Speakers 01 (ASIOVADPRO Driver)".into(),
            shutdown_after_clear: true,
            manage_defender: false,
            defender_images: DefenderImages {
                turn_on: "D:/shots/turn_on.png".into(),
                ..Default::default()
            },
            executables: Executables {
                gig_performer: "D:/apps/GigPerformer5.exe".into(),
                ..Default::default()
            },
            window_names: WindowNames {
                gig_performer: "Keyboard_Live".into(),
                ..Default::default()
            },
        }
    }

    /// A filled-in form with nothing chosen in Advanced. Each test overrides
    /// only the field it is about, so what it is testing is the only thing that
    /// differs from a plain save.
    fn form() -> FormInput<'static> {
        FormInput {
            phone: "Phone",
            hotspot: "Hotspot",
            osc_path: "p.json",
            midi_device: "x",
            audio_device: "y",
            defender_images: DefenderImages::default(),
            executables: Executables::default(),
            window_names: WindowNames::default(),
        }
    }

    #[test]
    fn the_chosen_audio_output_is_saved() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                midi_device: "Portable G-1 0",
                audio_device: "Haut-parleur (Realtek(R) Audio)",
                ..form()
            },
        );
        assert_eq!(saved.audio_device, "Haut-parleur (Realtek(R) Audio)");
    }

    #[test]
    fn an_empty_dropdown_keeps_the_stored_device() {
        // Nothing connected, so both dropdowns read empty.
        let saved = merge_from_form(
            stored(),
            FormInput {
                midi_device: "",
                audio_device: "",
                ..form()
            },
        );
        assert_eq!(saved.audio_device, "Speakers 01 (ASIOVADPRO Driver)");
        assert_eq!(saved.midi_device, "Portable G-1 0");
    }

    #[test]
    fn the_main_screen_toggles_survive_a_save() {
        let saved = merge_from_form(stored(), form());
        assert!(saved.shutdown_after_clear);
        assert!(!saved.manage_defender);
    }

    #[test]
    fn form_fields_are_trimmed() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                phone: "  Phone  ",
                hotspot: " Hotspot ",
                osc_path: " p.json ",
                ..form()
            },
        );
        assert_eq!(saved.phone_name, "Phone");
        assert_eq!(saved.hotspot_name, "Hotspot");
        assert_eq!(saved.osc_file_path, "p.json");
    }

    #[test]
    fn a_chosen_defender_image_is_saved() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                defender_images: DefenderImages {
                    turn_on: "D:/shots/new_turn_on.png".into(),
                    realtime_label: "D:/shots/new_realtime_label.png".into(),
                    ..Default::default()
                },
                ..form()
            },
        );
        assert_eq!(saved.defender_images.turn_on, "D:/shots/new_turn_on.png");
        assert_eq!(
            saved.defender_images.realtime_label,
            "D:/shots/new_realtime_label.png"
        );
    }

    /// Guards the Default button. Unlike the device dropdowns, an empty image
    /// field must not fall back to what was stored, or clearing a field would
    /// silently restore the path it was meant to remove.
    #[test]
    fn clearing_a_defender_image_restores_the_built_in_one() {
        let saved = merge_from_form(stored(), form());
        assert_eq!(
            saved.defender_images,
            DefenderImages::default(),
            "an empty image field fell back to the stored path, so Default cannot work"
        );
    }

    #[test]
    fn defender_image_paths_are_trimmed() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                defender_images: DefenderImages {
                    manage_settings: "  D:/shots/manage_settings.png  ".into(),
                    ..Default::default()
                },
                ..form()
            },
        );
        assert_eq!(
            saved.defender_images.manage_settings,
            "D:/shots/manage_settings.png"
        );
    }

    #[test]
    fn a_chosen_executable_is_saved() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                executables: Executables {
                    gig_performer: "D:/rigs/GigPerformer5.exe".into(),
                    open_stage_control: "D:/rigs/open-stage-control.exe".into(),
                },
                ..form()
            },
        );
        assert_eq!(saved.executables.gig_performer, "D:/rigs/GigPerformer5.exe");
        assert_eq!(
            saved.executables.open_stage_control,
            "D:/rigs/open-stage-control.exe"
        );
    }

    /// The same guard as the image fields: clearing an executable field has to
    /// mean "go back to the install path", not "keep the one on disk".
    #[test]
    fn clearing_an_executable_restores_the_built_in_path() {
        let saved = merge_from_form(stored(), form());
        assert_eq!(
            saved.executables,
            Executables::default(),
            "an empty executable field fell back to the stored path, so Default cannot work"
        );
    }

    #[test]
    fn executable_paths_are_trimmed() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                executables: Executables {
                    open_stage_control: "  D:/rigs/open-stage-control.exe  ".into(),
                    ..Default::default()
                },
                ..form()
            },
        );
        assert_eq!(
            saved.executables.open_stage_control,
            "D:/rigs/open-stage-control.exe"
        );
    }

    #[test]
    fn a_chosen_window_name_is_saved() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                window_names: WindowNames {
                    open_stage_control: "O-S-C".into(),
                    gig_performer: "Wedding_Set".into(),
                },
                ..form()
            },
        );
        assert_eq!(saved.window_names.open_stage_control, "O-S-C");
        assert_eq!(saved.window_names.gig_performer, "Wedding_Set");
    }

    /// The same guard as the images and the executables: clearing a window name
    /// has to mean "go back to the built-in one", not "keep the one on disk".
    #[test]
    fn clearing_a_window_name_restores_the_built_in_one() {
        let saved = merge_from_form(stored(), form());
        assert_eq!(
            saved.window_names,
            WindowNames::default(),
            "an empty window-name field fell back to the stored name, so Default cannot work"
        );
    }

    /// A stray space matters more here than in the path fields: the name is fed
    /// to a substring match, so " Keyboard_Live" would find nothing at all.
    #[test]
    fn window_names_are_trimmed() {
        let saved = merge_from_form(
            stored(),
            FormInput {
                window_names: WindowNames {
                    open_stage_control: "  Open Stage Control  ".into(),
                    gig_performer: " Keyboard_Live ".into(),
                },
                ..form()
            },
        );
        assert_eq!(saved.window_names.open_stage_control, "Open Stage Control");
        assert_eq!(saved.window_names.gig_performer, "Keyboard_Live");
    }

    /// A settings.json written before the Advanced section existed has neither
    /// block, and must read as "use the built-in defaults" rather than failing
    /// to parse and tripping the repair path.
    #[test]
    fn a_settings_file_without_the_image_block_reads_as_built_in() {
        let json = r#"{
            "phone_name": "MERBAC PRO",
            "hotspot_name": "MERBAC PRO",
            "osc_file_path": "D:/preset.json",
            "shutdown_after_clear": false,
            "midi_device": "Portable G-1 0",
            "manage_defender": true
        }"#;

        let parsed: Settings =
            serde_json::from_str(json).expect("an older settings.json must parse");
        assert_eq!(parsed.defender_images, DefenderImages::default());
        assert_eq!(parsed.executables, Executables::default());
        assert_eq!(parsed.window_names, WindowNames::default());
        assert_eq!(parsed.phone_name, "MERBAC PRO");
    }
}
