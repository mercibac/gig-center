//! Audio output watching, behind the activity LED at the bottom of the screen.
//!
//! Reads the endpoint's peak meter rather than capturing audio: no stream, no
//! extra crate, negligible cost. [`crate::utils::set_volume`] already performs
//! the same COM dance for the volume control.
//!
//! Polling runs on its own thread so the COM calls never touch the UI thread,
//! and it publishes a timestamp the UI reads — the same shape
//! [`crate::midi_input`] uses for MIDI.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

/// Idle endpoints read exactly 0.0 — measured across all five outputs on this
/// machine — while real playback peaked between 0.002 and 0.205. So the bar
/// only has to clear true silence, and sitting this low keeps quiet passages
/// from dropping the LED.
const SIGNAL_THRESHOLD: f32 = 0.002;

/// How long the LED stays lit after signal. Matches the MIDI LED so the two
/// behave alike, and bridges the gaps between peaks within a note.
const ACTIVITY_WINDOW_MS: u64 = 120;

/// How often the watcher thread reads the meter.
const POLL_INTERVAL: Duration = Duration::from_millis(30);

fn now_ms() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Whether a peak reading counts as audio playing.
pub fn is_signal(peak: f32) -> bool {
    peak > SIGNAL_THRESHOLD
}

/// COM has to be live on whichever thread touches these interfaces. The error
/// is ignored deliberately: COM may already be initialised, which is not a
/// failure.
unsafe fn ensure_com() {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
}

/// Read an endpoint's friendly name, or None if it will not give one up.
unsafe fn friendly_name(device: &windows::Win32::Media::Audio::IMMDevice) -> Option<String> {
    let store = device.OpenPropertyStore(STGM_READ).ok()?;
    // PROPVARIANT's Display goes through BSTR, which handles the VT_LPWSTR
    // these names arrive as — no manual PROPVARIANT unpacking needed.
    let name = store.GetValue(&PKEY_Device_FriendlyName).ok()?.to_string();
    (!name.is_empty()).then_some(name)
}

/// Friendly names of the audio outputs currently active.
pub fn list_devices() -> Vec<String> {
    unsafe {
        ensure_com();
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return Vec::new();
        };
        let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };

        (0..collection.GetCount().unwrap_or(0))
            .filter_map(|i| friendly_name(&collection.Item(i).ok()?))
            .collect()
    }
}

/// Pick the output to watch: the saved one when it is present, otherwise the
/// first available. None when the machine reports no outputs at all.
///
/// Free of COM on purpose, so the fallback is testable without hardware.
pub fn choose_device(saved: &str, available: &[String]) -> Option<String> {
    if !saved.is_empty() && available.iter().any(|name| name == saved) {
        return Some(saved.to_string());
    }
    available.first().cloned()
}

/// Watches one audio output and records when it last carried signal.
pub struct AudioMeter {
    pub device: Option<String>,
    last_signal_ms: Arc<AtomicU64>,
    /// Cleared to ask the watcher thread to stop.
    running: Arc<AtomicBool>,
}

impl AudioMeter {
    pub fn new() -> Self {
        Self {
            device: None,
            last_signal_ms: Arc::new(AtomicU64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Watch the named output. Returns whether a watcher started.
    ///
    /// Stops any watcher already running, so this doubles as "switch to".
    pub fn open(&mut self, device_name: &str) -> bool {
        self.close();
        if device_name.is_empty() {
            return false;
        }

        let wanted = device_name.to_string();
        let last = Arc::clone(&self.last_signal_ms);
        let running = Arc::new(AtomicBool::new(true));
        self.running = Arc::clone(&running);
        let watched = wanted.clone();

        thread::spawn(move || unsafe {
            ensure_com();
            let Some(meter) = meter_for(&watched) else {
                return;
            };
            while running.load(Ordering::Relaxed) {
                if let Ok(peak) = meter.GetPeakValue() {
                    if is_signal(peak) {
                        last.store(now_ms(), Ordering::Relaxed);
                    }
                }
                thread::sleep(POLL_INTERVAL);
            }
        });

        self.device = Some(wanted);
        true
    }

    /// Whether signal arrived recently enough to light the LED.
    pub fn is_active(&self) -> bool {
        let last = self.last_signal_ms.load(Ordering::Relaxed);
        last != 0 && now_ms().saturating_sub(last) < ACTIVITY_WINDOW_MS
    }

    /// Stop watching. Safe to call when nothing is running.
    pub fn close(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        self.device = None;
    }
}

/// Find the peak meter for a named output. Must run on a COM-initialised thread.
unsafe fn meter_for(device_name: &str) -> Option<IAudioMeterInformation> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
    let collection = enumerator
        .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        .ok()?;

    for i in 0..collection.GetCount().unwrap_or(0) {
        let Ok(device) = collection.Item(i) else {
            continue;
        };
        if friendly_name(&device).as_deref() == Some(device_name) {
            return device.Activate(CLSCTX_ALL, None).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_output_is_used_when_present() {
        let available = vec!["Realtek".to_string(), "ASIO 01".to_string()];
        assert_eq!(
            choose_device("ASIO 01", &available).as_deref(),
            Some("ASIO 01")
        );
    }

    #[test]
    fn falls_back_to_first_available() {
        let available = vec!["Realtek".to_string()];
        assert_eq!(
            choose_device("ASIO 01", &available).as_deref(),
            Some("Realtek")
        );
        assert_eq!(choose_device("", &available).as_deref(), Some("Realtek"));
    }

    #[test]
    fn no_output_when_none_are_active() {
        assert_eq!(choose_device("Realtek", &[]), None);
    }

    #[test]
    fn silence_does_not_count_as_signal() {
        // Every idle endpoint on this machine reported exactly 0.0.
        assert!(!is_signal(0.0));
    }

    #[test]
    fn real_playback_counts_as_signal() {
        // Peaks sampled while a wav played: 0.0110, 0.0464, 0.2046.
        assert!(is_signal(0.0110));
        assert!(is_signal(0.2046));
    }
}
