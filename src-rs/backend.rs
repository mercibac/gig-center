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

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

/// What the worker found.
#[derive(Clone, Default)]
pub struct Devices {
    pub midi: Vec<String>,
    pub audio: Vec<String>,
}

thread_local! {
    /// What to do with the devices once they exist, parked here until they do.
    ///
    /// Lives in a thread-local because applying them needs the `Rc<RefCell<…>>`
    /// state every callback shares, which cannot cross threads — the same
    /// reason, and the same shape, as `first_paint::run_after_first_frame`.
    static ON_READY: RefCell<Option<Box<dyn FnOnce(Devices)>>> = const { RefCell::new(None) };
}

/// Register what to do when the devices arrive. Call from the UI thread, and
/// before anything can deliver.
pub fn on_ready(handler: impl FnOnce(Devices) + 'static) {
    ON_READY.with(|slot| *slot.borrow_mut() = Some(Box::new(handler)));
}

/// Hand the devices over, if nobody has already.
///
/// Taking the handler out of the slot is what makes this exactly-once, and that
/// is the whole point: the quick path calls this directly, the worker's pushed
/// call calls it from the event loop, and a deadline may call it too. Whichever
/// arrives first wins and the rest are no-ops, with no flag to get wrong.
///
/// The previous design polled instead, and treated a closed channel as failure
/// — but a channel closes as a matter of course once the worker has delivered,
/// so every poll after the first re-applied empty device lists, printing "No
/// MIDI device connected." and closing both devices 33 times a second.
pub fn deliver(devices: Devices) {
    if let Some(handler) = ON_READY.with(|slot| slot.borrow_mut().take()) {
        handler(devices);
    }
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

        // Announced twice, on purpose, because the two readers want it at
        // different moments and neither knows which will get there first.
        //
        // The channel answers the question startup blocks on: "is this done
        // yet?". The pushed call is how the answer arrives when it is not —
        // queued onto the UI thread, run once the event loop is up. Both end at
        // `deliver`, which only lets one of them through.
        let _ = tx.send(devices.clone());
        let _ = slint::invoke_from_event_loop(move || deliver(devices));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

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
    fn the_devices_are_handed_over_exactly_once() {
        // The invariant the old polling design broke: it re-applied empty
        // device lists every 30ms after the first success, printing "No MIDI
        // device connected." forever and closing both devices with it.
        let handled = Rc::new(RefCell::new(Vec::new()));

        let record = Rc::clone(&handled);
        on_ready(move |found: Devices| record.borrow_mut().push(found.midi.len()));

        deliver(devices());
        deliver(devices());
        deliver(Devices::default());

        assert_eq!(
            *handled.borrow(),
            vec![1],
            "the handover ran more than once"
        );
    }

    #[test]
    fn delivering_with_nobody_waiting_does_nothing() {
        // The ordinary case once the quick path has already handled it: the
        // worker's pushed call still arrives, and must be a no-op rather than
        // an error or a second handover.
        ON_READY.with(|slot| *slot.borrow_mut() = None);

        deliver(devices());
    }
}
