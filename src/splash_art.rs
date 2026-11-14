//! The splash screen's artwork, compiled into the binary.
//!
//! Embedded rather than loaded from `assets/` for the reason spelled out in
//! [`crate::assets`]: the shipped exe is a single file started from wherever it
//! was copied to, and resolving asset paths against the working directory is
//! what made the Defender templates silently unavailable in release builds.
//!
//! Both images are produced by `assets/make_splash_logo.py`.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

static LOGO: &[u8] = include_bytes!("../assets/splash_logo.png");
static SHINE: &[u8] = include_bytes!("../assets/splash_shine.png");

/// Decode an embedded PNG into a Slint image.
///
/// None on failure, which leaves the splash without that piece of art rather
/// than taking the app down during startup.
fn decode(name: &str, bytes: &'static [u8]) -> Option<Image> {
    let decoded = match image::load_from_memory(bytes) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            console_log!("Error decoding embedded splash art {}: {}", name, e);
            return None;
        }
    };

    let (width, height) = decoded.dimensions();
    // from_rgba8, not the premultiplied variant: the generator writes straight
    // alpha, and the drop shadow reads muddy if it is treated as premultiplied.
    Some(Image::from_rgba8(
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(decoded.as_raw(), width, height),
    ))
}

/// The 3D logo.
pub fn logo() -> Option<Image> {
    decode("splash_logo.png", LOGO)
}

/// The highlight that sweeps across it.
pub fn shine() -> Option<Image> {
    decode("splash_shine.png", SHINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_images_decode_at_the_size_the_generator_writes() {
        // Guards against the artwork being replaced by something the splash
        // geometry no longer fits: splash.slint clips the shine to a circle
        // derived from the logo's canvas being square, with the face at a known
        // fraction of it.
        let logo = logo().expect("the embedded logo must decode");
        assert_eq!(logo.size().width, 640);
        assert_eq!(logo.size().height, 640);

        let shine = shine().expect("the embedded shine must decode");
        assert_eq!(shine.size().width, 190);
        assert_eq!(shine.size().height, 560);
    }
}
