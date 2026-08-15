//! Title bar / taskbar icon for the main window.
//!
//! Winit creates its windows without an icon, so Windows draws the generic
//! placeholder in the title bar even though icon.ico is embedded in the exe —
//! that resource is what Explorer and the shell read, not the window.
//! The icon must be part of the winit window attributes, and those are only
//! consumed when the event loop creates the real window, so it is installed as
//! a backend hook before the first window exists rather than set on MainWindow.

use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
use slint::winit_030::winit::window::Icon;

/// What Windows draws in the title bar (16px at 100% DPI, more when scaled).
const TITLE_BAR_SIZE: u32 = 32;

/// What Windows draws in the taskbar and the Alt+Tab switcher.
const TASKBAR_SIZE: u32 = 64;

/// Raw RGBA at the two sizes above, scaled by build.rs and embedded so the
/// running exe never depends on assets/icon.png being on disk. Pre-scaling keeps
/// the PNG decode and its two Lanczos passes out of startup entirely.
const TITLE_BAR_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_32.rgba"));
const TASKBAR_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_64.rgba"));

/// Add a hook that gives every window Slint creates the app icon. Must be
/// applied to the selector before the first window is created.
pub fn configure(selector: slint::BackendSelector) -> slint::BackendSelector {
    let Some((title_bar, taskbar)) = icons() else {
        // Nothing to configure without an icon — leave the selector untouched.
        return selector;
    };

    selector.with_winit_window_attributes_hook(move |attributes| {
        attributes
            .with_window_icon(Some(title_bar.clone()))
            .with_taskbar_icon(Some(taskbar.clone()))
    })
}

/// Wrap the pre-scaled pixels. Only a copy and a length check at this point.
fn icons() -> Option<(Icon, Icon)> {
    Some((
        Icon::from_rgba(TITLE_BAR_RGBA.to_vec(), TITLE_BAR_SIZE, TITLE_BAR_SIZE).ok()?,
        Icon::from_rgba(TASKBAR_RGBA.to_vec(), TASKBAR_SIZE, TASKBAR_SIZE).ok()?,
    ))
}
