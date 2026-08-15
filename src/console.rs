use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

static BUFFER: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static UNLOCK: AtomicBool = AtomicBool::new(false);

/// Append a message to the shared console buffer.
pub fn log(msg: String) {
    // Also print to real stderr for development debugging
    eprint!("{}", msg);
    // Recover from poisoning rather than dropping the message: the buffer holds
    // nothing but queued strings, and a worker that panicked is exactly when we
    // most need its logs to reach the UI.
    let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    buf.push(msg);
}

/// Drain all pending messages from the console buffer.
pub fn drain() -> Vec<String> {
    let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *buf)
}

/// Signal that the background task is done and buttons should unlock.
pub fn signal_unlock() {
    UNLOCK.store(true, Ordering::SeqCst);
}

/// Check and consume the unlock flag (returns true once, then resets).
pub fn should_unlock() -> bool {
    UNLOCK
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// Run a worker task, guaranteeing the UI unlock signal fires even if the task
/// panics. Nothing else ever sets the unlock flag, so a panic that escaped a
/// spawned worker used to leave the buttons disabled for the rest of the run.
///
/// Requires unwinding panics — see the `[profile.release]` note in Cargo.toml.
pub fn run_guarded<F: FnOnce()>(task_name: &str, task: F) {
    struct UnlockGuard;
    impl Drop for UnlockGuard {
        fn drop(&mut self) {
            signal_unlock();
        }
    }
    // Dropped after the panic is logged below, and also on any path that
    // somehow skips that logging.
    let _unlock = UnlockGuard;

    // AssertUnwindSafe: the task owns everything it touches, and the only state
    // it shares with the UI thread is the console buffer, which stays valid
    // through a panic (see `log`).
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task)) {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        crate::console_log!("\n--- {} FAILED: {} ---", task_name, msg);
    }
}

/// Macro to log messages to the UI console buffer.
/// Automatically appends a newline.
/// Usage: `console_log!("Hello, {}!", name);`
#[macro_export]
macro_rules! console_log {
    ($($arg:tt)*) => {
        $crate::console::log(format!("{}\n", format_args!($($arg)*)))
    };
}
