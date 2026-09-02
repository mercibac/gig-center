//! Template images the screen matcher searches for, compiled into the binary.
//!
//! The shipped exe is a single file, started from wherever it was copied to.
//! Loading these from `assets/` on disk resolved them against the working
//! directory, so a release build launched from anywhere but the repository root
//! reported "template image not found" and every Defender click silently did
//! nothing. Embedding removes that dependency, the same way [`crate::app_icon`]
//! embeds the window icon — together they are under 10 KB.

use image::GrayImage;

/// One template image, decoded on demand from bytes baked into the exe.
pub struct Template {
    /// Names the image in log messages; carries the old `assets/` prefix so the
    /// console reads the same as when these were loaded by path.
    pub name: &'static str,
    bytes: &'static [u8],
}

impl Template {
    /// Decode to the grayscale form [`crate::ncc`] matches against.
    pub fn to_luma8(&self) -> Option<GrayImage> {
        match image::load_from_memory(self.bytes) {
            Ok(img) => Some(img.to_luma8()),
            Err(e) => {
                console_log!("Error decoding embedded template {}: {}", self.name, e);
                None
            }
        }
    }
}

pub static DEFENDER_ACTIVATE: Template = Template {
    name: "assets/defender_activate_icon.png",
    bytes: include_bytes!("../assets/defender_activate_icon.png"),
};

pub static DEFENDER_PROTECTION: Template = Template {
    name: "assets/defender_protection_icon.png",
    bytes: include_bytes!("../assets/defender_protection_icon.png"),
};

pub static DEFENDER_SETTINGS: Template = Template {
    name: "assets/defender_settings_icon.png",
    bytes: include_bytes!("../assets/defender_settings_icon.png"),
};

pub static DEFENDER_REALTIME: Template = Template {
    name: "assets/defender_realtime_icon.png",
    bytes: include_bytes!("../assets/defender_realtime_icon.png"),
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the bug these constants exist to fix: a template that is missing
    /// from the binary or fails to decode leaves the Defender toggle clicking
    /// nothing, and the app only finds out at gig time.
    #[test]
    fn every_template_decodes_at_its_expected_size() {
        let expected = [
            (&DEFENDER_ACTIVATE, 245, 237),
            (&DEFENDER_PROTECTION, 242, 185),
            (&DEFENDER_SETTINGS, 166, 60),
            (&DEFENDER_REALTIME, 123, 48),
        ];

        for (template, width, height) in expected {
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
}
