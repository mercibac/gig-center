//! MIDI input watching, behind the activity LED on the main screen.
//!
//! midir delivers messages on its own thread, so nothing here touches the UI.
//! The listener only records *when* a message last arrived; the UI polls that
//! on its own timer and lights the LED. Ported from the Python prototype.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use midir::{Ignore, MidiInput, MidiInputConnection};

/// What other MIDI software sees this app called.
const CLIENT_NAME: &str = "Gig Control Center";

/// Status bytes from 0xF8 up are System Real-Time: MIDI clock, active sensing
/// and friends. Controllers stream those continuously whether or not anyone is
/// playing, so counting them as activity would leave the LED permanently lit
/// and telling you nothing.
const REALTIME_STATUS_FLOOR: u8 = 0xF8;

/// How long the LED stays lit after a message. Long enough to catch the eye on
/// a single note, short enough that playing reads as a flicker, not a hold.
const ACTIVITY_WINDOW_MS: u64 = 120;

/// Timestamps are milliseconds from the first call, so they fit an atomic that
/// the MIDI thread can write and the UI thread can read without a lock.
fn now_ms() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Whether a raw MIDI message means someone is actually playing.
pub fn is_activity(message: &[u8]) -> bool {
    match message.first() {
        Some(&status) => status < REALTIME_STATUS_FLOOR,
        None => false,
    }
}

/// Names of the MIDI input ports currently connected.
pub fn list_devices() -> Vec<String> {
    let Ok(midi_in) = MidiInput::new(CLIENT_NAME) else {
        return Vec::new();
    };
    midi_in
        .ports()
        .iter()
        .filter_map(|port| midi_in.port_name(port).ok())
        .collect()
}

/// Pick the port to listen on.
///
/// The saved device when it is plugged in, otherwise the first one that is — a
/// device named in settings.json may simply not be connected yet. None when
/// nothing is connected at all.
pub fn choose_device(saved: &str, available: &[String]) -> Option<String> {
    if !saved.is_empty() && available.iter().any(|name| name == saved) {
        return Some(saved.to_string());
    }
    available.first().cloned()
}

/// Watches one MIDI input port and records when it last saw real activity.
pub struct MidiListener {
    /// Dropping this closes the port, which is the whole of `close`.
    connection: Option<MidiInputConnection<()>>,
    pub device: Option<String>,
    /// Written by midir's thread, read by the UI timer. 0 means nothing yet.
    last_message_ms: Arc<AtomicU64>,
}

impl MidiListener {
    pub fn new() -> Self {
        Self {
            connection: None,
            device: None,
            last_message_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Listen on the named port. Returns whether it opened.
    ///
    /// Closes any port already open, so this doubles as "switch to". Whether a
    /// second program can hold the same device at once is up to the driver.
    pub fn open(&mut self, device_name: &str) -> bool {
        self.close();
        if device_name.is_empty() {
            return false;
        }

        let Ok(mut midi_in) = MidiInput::new(CLIENT_NAME) else {
            return false;
        };
        // Clock, sysex and active sensing never count as playing, and dropping
        // them here means they never reach the callback at all.
        midi_in.ignore(Ignore::All);

        let port = midi_in.ports().into_iter().find(|port| {
            midi_in
                .port_name(port)
                .is_ok_and(|name| name == device_name)
        });
        let Some(port) = port else {
            return false;
        };

        let last = Arc::clone(&self.last_message_ms);
        match midi_in.connect(
            &port,
            "gig-center-in",
            move |_timestamp, message, _| {
                if is_activity(message) {
                    last.store(now_ms(), Ordering::Relaxed);
                }
            },
            (),
        ) {
            Ok(connection) => {
                self.connection = Some(connection);
                self.device = Some(device_name.to_string());
                true
            }
            Err(e) => {
                console_log!("Could not open MIDI device '{}': {}", device_name, e);
                false
            }
        }
    }

    /// Whether a message arrived recently enough to light the LED.
    pub fn is_active(&self) -> bool {
        let last = self.last_message_ms.load(Ordering::Relaxed);
        last != 0 && now_ms().saturating_sub(last) < ACTIVITY_WINDOW_MS
    }

    /// Release the port. Safe to call when nothing is open.
    pub fn close(&mut self) {
        if let Some(connection) = self.connection.take() {
            connection.close();
        }
        self.device = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_device_is_used_when_connected() {
        let available = vec!["Launchkey 1".to_string(), "MPK mini 0".to_string()];
        assert_eq!(
            choose_device("MPK mini 0", &available).as_deref(),
            Some("MPK mini 0")
        );
    }

    #[test]
    fn falls_back_to_first_available_when_saved_is_absent() {
        // The rig is set up, but a different controller is plugged in today.
        let available = vec!["Launchkey 1".to_string()];
        assert_eq!(
            choose_device("MPK mini 0", &available).as_deref(),
            Some("Launchkey 1")
        );
    }

    #[test]
    fn falls_back_when_nothing_was_ever_saved() {
        let available = vec!["Launchkey 1".to_string()];
        assert_eq!(
            choose_device("", &available).as_deref(),
            Some("Launchkey 1")
        );
    }

    #[test]
    fn no_device_when_nothing_is_connected() {
        assert_eq!(choose_device("MPK mini 0", &[]), None);
    }

    #[test]
    fn playing_counts_as_activity() {
        assert!(is_activity(&[0x90, 60, 100])); // note on
        assert!(is_activity(&[0xB0, 7, 127])); // control change
    }

    #[test]
    fn realtime_traffic_does_not_count() {
        // Streamed forever whether or not anyone is playing; counting either
        // would pin the LED on.
        assert!(!is_activity(&[0xF8])); // clock
        assert!(!is_activity(&[0xFE])); // active sensing
        assert!(!is_activity(&[])); // nothing at all
    }
}
