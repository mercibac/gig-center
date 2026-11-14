//! Template images the screen matcher searches for, compiled into the binary.
//!
//! The shipped exe is a single file, started from wherever it was copied to.
//! Loading these from `assets/` on disk resolved them against the working
//! directory, so a release build launched from anywhere but the repository root
//! reported "template image not found" and every Defender click silently did
//! nothing. Embedding removes that dependency, the same way [`crate::app_icon`]
//! embeds the window icon — together they are under 10 KB.
//!
//! Embedding is still what ships and still the default, but each of the four can
//! be pointed at a file instead from the Advanced section of the settings modal:
//! these are screenshots of one particular Defender UI, and a different Windows
//! build, language or display scaling needs different ones. A chosen image is
//! read when it is matched rather than at startup, so replacing the file takes
//! effect on the next toggle, and anything that stops it loading falls back to
//! the embedded image instead of leaving the automation clicking nothing — a gig
//! is not the moment to find out a template has gone. See [`Template::to_luma8`].

use crate::settings::DefenderImages;
use image::GrayImage;
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

/// Where a template's pixels come from.
enum Source {
    /// Baked into the exe by `include_bytes!`.
    Embedded(&'static [u8]),
    /// Chosen in the Advanced settings, read from disk when it is matched.
    File(PathBuf),
}

/// One template image, decoded on demand.
pub struct Template {
    /// Names the image in log messages. A built-in one carries the old
    /// `assets/` prefix so the console reads the same as when these were loaded
    /// by path; a chosen one carries its own path, so the log says which image
    /// was actually searched for.
    pub name: Cow<'static, str>,
    source: Source,
    /// The built-in image to fall back on when `source` is a [`Source::File`]
    /// that will not load. `None` for the built-in templates themselves.
    fallback: Option<&'static [u8]>,
}

impl Template {
    /// Decode to the grayscale form [`crate::ncc`] matches against.
    pub fn to_luma8(&self) -> Option<GrayImage> {
        match &self.source {
            Source::Embedded(bytes) => decode(bytes, &self.name),
            Source::File(path) => match fs::read(path) {
                Ok(bytes) => decode(&bytes, &self.name).or_else(|| self.built_in_fallback()),
                Err(e) => {
                    console_log!("Could not read the chosen image {} ({e}).", path.display());
                    self.built_in_fallback()
                }
            },
        }
    }

    /// The embedded image, for when the chosen one is missing or unreadable.
    /// Falling back keeps the Defender step working on the original template
    /// rather than failing the whole toggle.
    fn built_in_fallback(&self) -> Option<GrayImage> {
        let bytes = self.fallback?;
        console_log!("Falling back to the built-in image for this step.");
        decode(bytes, &self.name)
    }
}

/// Decode image bytes to grayscale, naming the template if they are not an image.
fn decode(bytes: &[u8], name: &str) -> Option<GrayImage> {
    match image::load_from_memory(bytes) {
        Ok(img) => Some(img.to_luma8()),
        Err(e) => {
            console_log!("Error decoding template {name}: {e}");
            None
        }
    }
}

// The log name and bytes of each built-in template, kept as consts so both the
// statics below and `defender_template` can reach them.
const DEACTIVATION_STATE_NAME: &str = "assets/defender_activate_icon.png";
const DEACTIVATION_STATE_BYTES: &[u8] = include_bytes!("../assets/defender_activate_icon.png");
const ACTIVATION_STATE_NAME: &str = "assets/defender_protection_icon.png";
const ACTIVATION_STATE_BYTES: &[u8] = include_bytes!("../assets/defender_protection_icon.png");
const MANAGE_SETTINGS_NAME: &str = "assets/defender_settings_icon.png";
const MANAGE_SETTINGS_BYTES: &[u8] = include_bytes!("../assets/defender_settings_icon.png");
const REALTIME_LABEL_NAME: &str = "assets/defender_realtime_icon.png";
const REALTIME_LABEL_BYTES: &[u8] = include_bytes!("../assets/defender_realtime_icon.png");

/// The Defender steps whose template image can be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenderSlot {
    DeactivationState,
    ActivationState,
    RealtimeLabel,
    ManageSettings,
}

impl DefenderSlot {
    /// The log name and bytes of the image compiled into the exe for this step.
    fn built_in(self) -> (&'static str, &'static [u8]) {
        match self {
            Self::DeactivationState => (DEACTIVATION_STATE_NAME, DEACTIVATION_STATE_BYTES),
            Self::ActivationState => (ACTIVATION_STATE_NAME, ACTIVATION_STATE_BYTES),
            Self::RealtimeLabel => (REALTIME_LABEL_NAME, REALTIME_LABEL_BYTES),
            Self::ManageSettings => (MANAGE_SETTINGS_NAME, MANAGE_SETTINGS_BYTES),
        }
    }

    /// The path chosen for this step, empty when there is none.
    fn chosen(self, images: &DefenderImages) -> &str {
        match self {
            Self::DeactivationState => &images.deactivation_state,
            Self::ActivationState => &images.activation_state,
            Self::RealtimeLabel => &images.realtime_label,
            Self::ManageSettings => &images.manage_settings,
        }
    }
}

/// The template to match for `slot`, honouring a chosen image when there is one.
///
/// Returns an owned [`Template`] rather than a reference to one of the statics,
/// because a chosen path is only known at runtime. [`crate::screen`] takes
/// `&Template` either way, so nothing there had to change.
pub fn defender_template(slot: DefenderSlot, images: &DefenderImages) -> Template {
    let (built_in_name, built_in_bytes) = slot.built_in();
    let chosen = slot.chosen(images).trim();

    if chosen.is_empty() {
        return Template {
            name: Cow::Borrowed(built_in_name),
            source: Source::Embedded(built_in_bytes),
            fallback: None,
        };
    }

    Template {
        name: Cow::Owned(chosen.to_string()),
        source: Source::File(PathBuf::from(chosen)),
        fallback: Some(built_in_bytes),
    }
}

/// Look at a chosen image the way the matcher will: that it is there, that it
/// decodes, and how big it is.
///
/// Reported in the console when settings are saved, so a path that will not work
/// surfaces at the desk rather than mid-gig — the same failure the embedding
/// above was introduced to stop.
pub fn inspect_custom_image(path: &str) -> Result<(u32, u32), String> {
    let bytes = fs::read(Path::new(path)).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    Ok((img.width(), img.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch path of its own per test. There is no `tempfile`
    /// dev-dependency and these tests do not need one.
    fn scratch(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("gig-center-{tag}-{unique}.png"))
    }

    /// A flat grey PNG of a known size, standing in for a replacement template.
    fn write_png(path: &Path, width: u32, height: u32) {
        GrayImage::from_pixel(width, height, image::Luma([128u8]))
            .save(path)
            .expect("could not write the test image");
    }

    /// Guards the bug these constants exist to fix: a template that is missing
    /// from the binary or fails to decode leaves the Defender toggle clicking
    /// nothing, and the app only finds out at gig time.
    #[test]
    fn every_template_decodes_at_its_expected_size() {
        let expected = [
            (DefenderSlot::DeactivationState, 245, 237),
            (DefenderSlot::ActivationState, 242, 185),
            (DefenderSlot::ManageSettings, 166, 60),
            (DefenderSlot::RealtimeLabel, 123, 48),
        ];

        for (slot, width, height) in expected {
            let template = defender_template(slot, &DefenderImages::default());
            let img = template
                .to_luma8()
                .unwrap_or_else(|| panic!("{} failed to decode", template.name));
            assert_eq!(
                (img.width(), img.height()),
                (width, height),
                "{} decoded at the wrong size",
                template.name
            );
        }
    }

    #[test]
    fn no_chosen_image_uses_the_one_in_the_exe() {
        let template =
            defender_template(DefenderSlot::DeactivationState, &DefenderImages::default());
        let img = template.to_luma8().expect("the built-in image must decode");
        assert_eq!((img.width(), img.height()), (245, 237));
        assert_eq!(template.name, DEACTIVATION_STATE_NAME);
    }

    /// Whitespace round a path should not count as a choice, so a field left
    /// holding a stray space still resolves to the built-in image.
    #[test]
    fn a_blank_chosen_image_uses_the_one_in_the_exe() {
        let images = DefenderImages {
            deactivation_state: "   ".into(),
            ..Default::default()
        };
        let template = defender_template(DefenderSlot::DeactivationState, &images);
        let img = template.to_luma8().expect("the built-in image must decode");
        assert_eq!((img.width(), img.height()), (245, 237));
    }

    #[test]
    fn a_chosen_image_is_matched_instead_of_the_built_in_one() {
        let path = scratch("chosen");
        write_png(&path, 60, 40);

        let images = DefenderImages {
            deactivation_state: path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let template = defender_template(DefenderSlot::DeactivationState, &images);
        let img = template.to_luma8().expect("the chosen image must decode");

        assert_eq!(
            (img.width(), img.height()),
            (60, 40),
            "the built-in image was searched for instead of the chosen one"
        );
        let _ = fs::remove_file(&path);
    }

    /// A path that has gone away — the file moved, or the drive is not mounted —
    /// must not take the Defender step down with it.
    #[test]
    fn a_missing_chosen_image_falls_back_to_the_one_in_the_exe() {
        let images = DefenderImages {
            realtime_label: scratch("missing").to_string_lossy().to_string(),
            ..Default::default()
        };
        let template = defender_template(DefenderSlot::RealtimeLabel, &images);
        let img = template
            .to_luma8()
            .expect("a missing chosen image must fall back, not give up");
        assert_eq!((img.width(), img.height()), (123, 48));
    }

    #[test]
    fn a_corrupt_chosen_image_falls_back_to_the_one_in_the_exe() {
        let path = scratch("corrupt");
        fs::write(&path, b"this is not an image").expect("could not write the test file");

        let images = DefenderImages {
            manage_settings: path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let template = defender_template(DefenderSlot::ManageSettings, &images);
        let img = template
            .to_luma8()
            .expect("a corrupt chosen image must fall back, not give up");

        assert_eq!((img.width(), img.height()), (166, 60));
        let _ = fs::remove_file(&path);
    }

    /// The console line has to name the image that was really searched for, or a
    /// failed match sends you looking at the wrong file.
    #[test]
    fn the_console_names_the_chosen_image() {
        let images = DefenderImages {
            activation_state: "D:/shots/mine.png".into(),
            ..Default::default()
        };
        let template = defender_template(DefenderSlot::ActivationState, &images);
        assert_eq!(template.name, "D:/shots/mine.png");
    }

    #[test]
    fn inspecting_a_chosen_image_reports_its_size() {
        let path = scratch("inspect");
        write_png(&path, 31, 17);

        let size = inspect_custom_image(&path.to_string_lossy()).expect("the image must inspect");
        assert_eq!(size, (31, 17));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn inspecting_a_missing_image_explains_itself() {
        let err = inspect_custom_image(&scratch("nope").to_string_lossy())
            .expect_err("a missing image must report an error");
        assert!(!err.is_empty(), "the error gave no reason");
    }
}
