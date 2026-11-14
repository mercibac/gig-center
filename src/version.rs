//! The version string shown in the corner of the main window.
//!
//! Cargo insists on three components and the label wants two: `1.0.0` is what
//! `Cargo.toml` carries, what the tag and the installer and the Scoop manifest
//! all agree on, while the window reads `V1.0`. The trim happens here and
//! nowhere else, and it reads `CARGO_PKG_VERSION` rather than a literal — a
//! hardcoded string is a number that drifts from the release it shipped in the
//! first time someone bumps one and forgets the other.

/// What the bottom-row label shows, e.g. `V1.0`.
pub fn display() -> String {
    label(env!("CARGO_PKG_VERSION"))
}

/// Drop a `.0` patch, so `1.0.0` reads as `V1.0`. A release that actually
/// carries a patch keeps it: `1.2.3` stays `V1.2.3`.
fn label(version: &str) -> String {
    format!("V{}", version.strip_suffix(".0").unwrap_or(version))
}

#[cfg(test)]
mod tests {
    use super::{display, label};

    #[test]
    fn a_zero_patch_is_dropped() {
        assert_eq!(label("1.0.0"), "V1.0");
        assert_eq!(label("1.10.0"), "V1.10");
        assert_eq!(label("10.0.0"), "V10.0");
    }

    /// Only the patch goes. `1.0.10` ends in a zero digit but not in `.0`, and
    /// trimming it would advertise the wrong release.
    #[test]
    fn a_real_patch_is_kept() {
        assert_eq!(label("1.2.3"), "V1.2.3");
        assert_eq!(label("1.0.10"), "V1.0.10");
    }

    /// The whole point of reading the manifest: whatever `Cargo.toml` says is
    /// what the window says.
    #[test]
    fn display_follows_the_manifest() {
        assert_eq!(display(), label(env!("CARGO_PKG_VERSION")));
    }
}
