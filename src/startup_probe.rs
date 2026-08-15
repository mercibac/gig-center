//! Phase-by-phase startup instrumentation. Compiled in, but completely inert
//! unless `GIG_STARTUP_PROBE` is set to a log file path.
//!
//! Existing timing tools all measure the wrong thing here: the process spends
//! most of its life in the event loop, so "how long did the exe run" says
//! nothing, and an external poller only sees the window handle appear, which
//! happens hundreds of milliseconds before the window has pixels. When enabled,
//! the app appends one line of phase timings to the log and quits as soon as the
//! first frame is on screen.
//!
//! Timings start at process *creation*, not at `main`, so the loader cost of
//! pulling in the graphics DLLs is counted the way the user experiences it.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::winit::event_loop::ActiveEventLoop;
use slint::winit_030::winit::window::WindowId;
use slint::winit_030::{CustomApplicationHandler, EventResult};

struct Probe {
    log: PathBuf,
    entry: Instant,
    last: Instant,
    /// Process creation until `main` was entered — image load and DLL resolution.
    preload_ms: f64,
    phases: Vec<(&'static str, f64)>,
    /// First completed draw. Exact for every renderer — see [`FrameWatch`].
    frame_ms: Option<f64>,
    /// End of the first render pass, as reported by the renderer itself. Only
    /// the GPU renderers implement this; kept as a cross-check on `frame_ms`.
    painted_ms: Option<f64>,
    /// Signals still expected before the run can be reported.
    pending: u8,
}

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);

/// Whether instrumentation is active. Cheap enough to call on any path.
pub fn enabled() -> bool {
    PROBE.lock().is_ok_and(|p| p.is_some())
}

/// Begin measuring. Call as the first statement of `main`.
pub fn start() {
    let Some(log) = std::env::var_os("GIG_STARTUP_PROBE") else {
        return;
    };
    let now = Instant::now();
    *PROBE.lock().unwrap() = Some(Probe {
        log: log.into(),
        entry: now,
        last: now,
        preload_ms: preload_ms().unwrap_or(f64::NAN),
        phases: Vec::new(),
        frame_ms: None,
        painted_ms: None,
        pending: 0,
    });
}

/// Close off the phase that just finished and open the next one.
pub fn mark(name: &'static str) {
    let mut guard = PROBE.lock().unwrap();
    let Some(probe) = guard.as_mut() else {
        return;
    };
    let now = Instant::now();
    probe.phases.push((name, (now - probe.last).as_secs_f64() * 1000.0));
    probe.last = now;
}

/// Install the event loop hook that detects the first completed frame.
pub fn configure(selector: slint::BackendSelector) -> slint::BackendSelector {
    let mut guard = PROBE.lock().unwrap();
    let Some(probe) = guard.as_mut() else {
        return selector;
    };
    probe.pending += 1;
    drop(guard);
    selector.with_winit_custom_application_handler(FrameWatch { drawn: false })
}

/// Ask the renderer to report its own first frame, as a cross-check. Not every
/// renderer supports this, so it is additional evidence rather than the signal.
pub fn watch_first_frame(window: &slint::Window) {
    if !enabled() {
        return;
    }
    let hooked = window
        .set_rendering_notifier(|state, _| {
            if matches!(state, slint::RenderingState::AfterRendering) {
                record(|probe| &mut probe.painted_ms);
            }
        })
        .is_ok();
    if hooked {
        PROBE.lock().unwrap().as_mut().unwrap().pending += 1;
    }
}

/// Watches the winit event loop for the first frame to be fully drawn.
///
/// `RedrawRequested` is handled by drawing synchronously, so by the time the
/// loop is about to wait again, the pixels are on screen. Unlike the renderer's
/// own notifier this holds for the software renderer too, which is the whole
/// point — it is the renderer being compared.
struct FrameWatch {
    drawn: bool,
}

impl CustomApplicationHandler for FrameWatch {
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _winit_window: Option<&slint::winit_030::winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &WindowEvent,
    ) -> EventResult {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.drawn = true;
        }
        EventResult::Propagate
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) -> EventResult {
        if self.drawn {
            record(|probe| &mut probe.frame_ms);
        }
        EventResult::Propagate
    }
}

/// Store one arrival time, and report once every expected signal has arrived.
fn record(slot: impl Fn(&mut Probe) -> &mut Option<f64>) {
    let mut guard = PROBE.lock().unwrap();
    let Some(probe) = guard.as_mut() else {
        return;
    };
    let ms = probe.entry.elapsed().as_secs_f64() * 1000.0;
    let field = slot(probe);
    if field.is_some() {
        return; // A repeat of a signal already seen — later frames, say.
    }
    *field = Some(ms);
    probe.pending -= 1;
    if probe.pending > 0 {
        return;
    }
    let probe = guard.take().expect("checked present above");
    drop(guard);
    write(&probe);
    slint::quit_event_loop().ok();
}

/// Append one line of timings to the log.
fn write(probe: &Probe) {
    let known: f64 = probe.phases.iter().map(|(_, ms)| ms).sum();
    let frame = probe.frame_ms.unwrap_or(f64::NAN);

    let mut line = format!("preload={:.1}", probe.preload_ms);
    for (name, ms) in &probe.phases {
        line.push_str(&format!(" {name}={ms:.1}"));
    }
    line.push_str(&format!(
        " event_loop={:.1} | frame={frame:.1} painted={} => TOTAL={:.1}\n",
        frame - known,
        probe.painted_ms.map_or("-".into(), |ms| format!("{ms:.1}")),
        probe.preload_ms + frame,
    ));

    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe.log)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Milliseconds between this process being created and now.
fn preload_ms() -> Option<f64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let ticks = |ft: FILETIME| ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;

    unsafe {
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .ok()?;
        // Both are UTC FILETIMEs in 100ns ticks.
        let elapsed = ticks(GetSystemTimeAsFileTime()).checked_sub(ticks(created))?;
        Some(elapsed as f64 / 10_000.0)
    }
}
