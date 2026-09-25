//! Keeps the window out of composition until it has a frame to show.
//!
//! Slint maps the window before anything has been drawn into it: showing is the
//! last step of window setup, and the first paint only happens once the event
//! loop delivers the WM_PAINT that showing produced. In between, the compositor
//! has nothing to draw but the window's untouched redirection surface, which
//! comes up white — the flash this module exists to remove. It is short, one or
//! two compositor frames, and impossible not to notice against a dark UI.
//!
//! Hiding the window outright is not the answer: Windows doesn't paint hidden
//! windows, so the frame being waited for would never arrive. Cloaking is. A
//! cloaked window is created, shown, sized and painted exactly as before and is
//! merely left out of what the compositor draws, so uncloaking it is a reveal of
//! a frame that is already finished.
//!
//! The cloak has to be in place before the window is mapped, and mapping is
//! buried inside Slint's window setup, so it is applied from a message hook on
//! the window's own `WM_CREATE` — the first moment DWM will accept the
//! attribute, and still a long way before anything is shown. The hook is scoped
//! to this thread and removes itself as soon as it has seen the window.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Duration;

use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::winit::event_loop::ActiveEventLoop;
use slint::winit_030::winit::window::WindowId;
use slint::winit_030::{CustomApplicationHandler, EventResult};

use windows::Win32::Foundation::{BOOL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, CREATESTRUCTW, CWPSTRUCT, HHOOK,
    WH_CALLWNDPROC, WINDOW_STYLE, WM_CREATE, WS_CAPTION, WS_CHILD,
};

/// The cloaked window, or 0 while there isn't one. Raw, because `HWND` is a
/// pointer and this has to live in a static.
static CLOAKED: AtomicIsize = AtomicIsize::new(0);

/// The message hook while it is installed, or 0.
static HOOK: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    /// Work deferred until the window is on screen. Anything slow that the
    /// first frame does not need belongs here: run before the frame it would
    /// delay the window appearing, and a timer could fire either side of it.
    static AFTER_FIRST_FRAME: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

/// Run `work` once the window has its first frame on screen. Call before the
/// event loop starts; runs on the UI thread, so it may touch the window.
pub fn run_after_first_frame(work: impl FnOnce() + 'static) {
    AFTER_FIRST_FRAME.with(|slot| *slot.borrow_mut() = Some(Box::new(work)));
}

/// Take the deferred work, if any is still waiting.
fn take_deferred_work() -> Option<Box<dyn FnOnce()>> {
    AFTER_FIRST_FRAME.with(|slot| slot.borrow_mut().take())
}

/// Set once the window has been revealed, after which both the hook and
/// [`reveal`] do nothing. Guards against cloaking a window that no one is left
/// to uncloak.
static REVEALED: AtomicBool = AtomicBool::new(false);

/// Arm the cloak again, for the next window to be created.
///
/// This module was written when there was one window. With a splash there are
/// two, each needing the same treatment, and which one the creation hook claims
/// is not something the caller controls: measured, winit creates the splash's
/// OS window first even when the main window is shown first. So rather than
/// trying to steer the hook, each window arms it in turn — the splash before it
/// is shown, the main window before it replaces the splash.
pub fn rearm() {
    REVEALED.store(false, Ordering::Relaxed);
    start();
}

/// Arrange for the next window created on this thread to come up cloaked. Call
/// before anything creates a window; the window itself is created much later,
/// from inside the event loop.
pub fn start() {
    let hook = unsafe {
        SetWindowsHookExW(
            WH_CALLWNDPROC,
            Some(on_window_created),
            HINSTANCE::default(),
            GetCurrentThreadId(),
        )
    };
    // Without the hook the window is simply never cloaked, and the flash stays.
    if let Ok(hook) = hook {
        HOOK.store(hook.0 as isize, Ordering::Relaxed);
    }
}

/// Install the event loop hook that reveals the window after its first frame.
pub fn configure(selector: slint::BackendSelector) -> slint::BackendSelector {
    selector.with_winit_custom_application_handler(RevealOnFirstFrame {
        seen: Vec::new(),
        pending_reveal: false,
        revealed_any: false,
    })
}

/// Start the safety net that reveals the window even if no frame arrives. Needs
/// a backend, so call it after one has been selected and before the event loop
/// runs.
///
/// `within` is how long to wait. Only reachable if the reveal never happens —
/// an unrevealed window would be an app that never appears, which is worse than
/// the flash this module exists to remove. With a splash holding the window the
/// wait has to be generous, because it is the backend being waited on and a
/// cold boot is slow.
pub fn arm_deadline(within: Duration) {
    slint::Timer::single_shot(within, reveal);
}

/// Put the window into composition, if it isn't there already. Idempotent, and
/// harmless when nothing was ever cloaked.
pub fn reveal() {
    if REVEALED.swap(true, Ordering::Relaxed) {
        return;
    }
    remove_hook();
    let cloaked = CLOAKED.swap(0, Ordering::Relaxed);
    if cloaked != 0 {
        set_cloaked(HWND(cloaked as *mut std::ffi::c_void), false);
    }
}

/// Reveals the window once its first frame has been drawn.
///
/// `RedrawRequested` is handled by drawing synchronously, and this handler runs
/// before Slint's, so the frame is only guaranteed to be finished by the time
/// the loop is about to wait again — the same signal [`crate::startup_probe`]
/// times, which is why it is reported from here rather than measured twice.
struct RevealOnFirstFrame {
    /// Windows that have had their first frame. Each is centred and revealed
    /// once, on its own first frame: with a splash there are two, and a single
    /// flag would serve whichever drew first and leave the other off-centre and
    /// cloaked.
    seen: Vec<WindowId>,
    /// A window drew for the first time and is waiting to be put into
    /// composition, once the loop is about to wait and the frame is finished.
    pending_reveal: bool,
    /// The deferred work runs once per arming, not once per process: the splash
    /// arms it for startup, and the handover arms it again to dismiss itself.
    revealed_any: bool,
}

impl CustomApplicationHandler for RevealOnFirstFrame {
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        winit_window: Option<&slint::winit_030::winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &WindowEvent,
    ) -> EventResult {
        if matches!(event, WindowEvent::RedrawRequested) {
            // Centring lives here because this is the one place holding the
            // winit window, and — for the main window — the last moment it is
            // still cloaked, so moving it is a move nobody sees. The size is
            // final by the first frame; before it, there is nothing to centre.
            if !self.seen.contains(&window_id) {
                self.seen.push(window_id);
                if let Some(window) = winit_window {
                    crate::center::on_primary_monitor(window);
                }
                self.pending_reveal = true;
            }
        }
        EventResult::Propagate
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) -> EventResult {
        if self.pending_reveal {
            self.pending_reveal = false;
            reveal();
            if !self.revealed_any {
                self.revealed_any = true;
                crate::startup_probe::note_first_frame();
            }

            // Only now that the window is up, and the startup timing is
            // recorded, is it safe to spend time on anything else.
            if let Some(work) = take_deferred_work() {
                work();
            }
        }
        EventResult::Propagate
    }
}

/// Cloaks the app window as it is being created, then takes itself out of the
/// way. Runs on the main thread, from inside `CreateWindowExW`, and sees every
/// message sent to this thread's windows until it does.
unsafe extern "system" fn on_window_created(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && !REVEALED.load(Ordering::Relaxed) && CLOAKED.load(Ordering::Relaxed) == 0 {
        let sent = &*(lparam.0 as *const CWPSTRUCT);
        if sent.message == WM_CREATE {
            let attributes = &*(sent.lParam.0 as *const CREATESTRUCTW);
            let style = WINDOW_STYLE(attributes.style as u32);

            // Everything else winit puts on this thread is either a child or
            // its message-only event target, and neither carries a caption.
            let is_app_window = attributes.hwndParent.is_invalid()
                && !style.contains(WS_CHILD)
                && style.contains(WS_CAPTION);

            if is_app_window && set_cloaked(sent.hwnd, true) {
                CLOAKED.store(sent.hwnd.0 as isize, Ordering::Relaxed);
                remove_hook();
            }
        }
    }

    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// Take the window in or out of what the compositor draws. Reports whether it
/// worked, so a failed cloak isn't remembered as one that has to be undone.
fn set_cloaked(window: HWND, cloaked: bool) -> bool {
    let value = BOOL::from(cloaked);
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_CLOAK,
            std::ptr::addr_of!(value).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
    }
    .is_ok()
}

/// Uninstall the message hook if it is still installed. Safe to call from
/// inside the hook itself.
fn remove_hook() {
    let hook = HOOK.swap(0, Ordering::Relaxed);
    if hook != 0 {
        unsafe { UnhookWindowsHookEx(HHOOK(hook as *mut std::ffi::c_void)).ok() };
    }
}
