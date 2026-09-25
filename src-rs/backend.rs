//! Device enumeration, off the UI thread.
//!
//! Listing devices is the slow part of starting up — the first `MidiInput` in a
//! process pays for initialising the MIDI subsystem, measured at ~90-105ms — and
//! it is pure data, touching no Slint object. Running it on a worker is what
//! makes it possible to ask "is the backend ready yet?" at all: done on the UI
//! thread, the question could not be asked until the answer was already yes.
//!
//! Opening the chosen device stays on the UI thread. It is cheap once the
//! subsystem is warm, and it owns Slint models and the `Rc<RefCell<…>>` state
//! every callback shares.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

/// What the worker found.
pub struct Devices {
    pub midi: Vec<String>,
    pub audio: Vec<String>,
}

/// How the probe is getting on.
pub enum Probe {
    /// Finished, with what it found.
    Ready(Devices),
    /// Still working.
    Pending,
    /// The worker is gone without answering. Startup carries on with nothing
    /// enumerated rather than waiting for something that will never arrive.
    Failed,
}

/// Start listing devices on a worker thread.
///
/// Call as early as possible: everything the UI does while this runs is time
/// the probe gets for free.
pub fn probe() -> Receiver<Devices> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let devices = Devices {
            midi: crate::midi_input::list_devices(),
            audio: crate::audio_input::list_devices(),
        };
        // Fails only if startup was abandoned and the receiver dropped, in
        // which case there is nobody left to tell.
        let _ = tx.send(devices);
    });
    rx
}

/// Wait up to `grace` for the probe to finish.
///
/// This is the whole decision: `Ready` means the splash is not needed, anything
/// else means it is. Blocking here is safe because it happens before any window
/// is shown — there is nothing on screen to freeze.
pub fn wait_for(rx: &Receiver<Devices>, grace: Duration) -> Probe {
    match rx.recv_timeout(grace) {
        Ok(devices) => Probe::Ready(devices),
        Err(RecvTimeoutError::Timeout) => Probe::Pending,
        Err(RecvTimeoutError::Disconnected) => Probe::Failed,
    }
}

/// Check the probe without waiting, for the splash to poll.
pub fn poll(rx: &Receiver<Devices>) -> Probe {
    match rx.try_recv() {
        Ok(devices) => Probe::Ready(devices),
        Err(TryRecvError::Empty) => Probe::Pending,
        Err(TryRecvError::Disconnected) => Probe::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Devices {
        Devices {
            midi: vec!["Portable G-1".to_string()],
            audio: vec!["Casque (Realtek(R) Audio)".to_string()],
        }
    }

    #[test]
    fn an_answer_inside_the_grace_period_means_no_splash() {
        let (tx, rx) = mpsc::channel();
        tx.send(devices()).unwrap();

        match wait_for(&rx, Duration::from_millis(100)) {
            Probe::Ready(found) => {
                assert_eq!(found.midi.len(), 1);
                assert_eq!(found.audio.len(), 1);
            }
            _ => panic!("a probe that already answered must count as ready"),
        }
    }

    #[test]
    fn silence_past_the_grace_period_means_show_the_splash() {
        // Held, not dropped: the worker is still going, which is exactly the
        // cold-boot case the splash exists for.
        let (_tx, rx) = mpsc::channel::<Devices>();

        assert!(matches!(
            wait_for(&rx, Duration::from_millis(20)),
            Probe::Pending
        ));
    }

    #[test]
    fn a_dead_worker_does_not_strand_startup() {
        let (tx, rx) = mpsc::channel::<Devices>();
        drop(tx);

        assert!(matches!(
            wait_for(&rx, Duration::from_millis(100)),
            Probe::Failed
        ));
    }

    #[test]
    fn polling_reports_the_same_three_states() {
        let (tx, rx) = mpsc::channel();
        assert!(matches!(poll(&rx), Probe::Pending));

        tx.send(devices()).unwrap();
        assert!(matches!(poll(&rx), Probe::Ready(_)));

        drop(tx);
        assert!(matches!(poll(&rx), Probe::Failed));
    }
}
