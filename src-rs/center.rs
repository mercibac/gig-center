//! Puts the main window in the middle of the screen.
//!
//! The move waits for the first frame rather than happening at startup: the
//! window is sized from the `preferred-width`/`preferred-height` in app.slint,
//! and that size is only settled once Slint has laid the window out. Centring
//! before then would centre the wrong rectangle.
//!
//! Waiting costs nothing visually, because [`crate::first_paint`] still has the
//! window cloaked at that point — it is moved before the compositor ever draws
//! it, so there is no jump to see.

use slint::winit_030::winit::dpi::PhysicalPosition;
use slint::winit_030::winit::window::Window;

/// Centre the window on the primary monitor.
///
/// Sizes come from `outer_size`, which includes the border and title bar, so
/// the whole window is centred rather than just its contents. Does nothing if
/// no monitor can be identified — an off-centre window beats a panic during
/// startup.
pub fn on_primary_monitor(window: &Window) {
    let Some(monitor) = window
        .primary_monitor()
        .or_else(|| window.current_monitor())
    else {
        return;
    };

    let screen = monitor.size();
    let origin = monitor.position();
    let frame = window.outer_size();

    window.set_outer_position(PhysicalPosition::new(
        origin.x + (screen.width as i32 - frame.width as i32) / 2,
        origin.y + (screen.height as i32 - frame.height as i32) / 2,
    ));
}
