"""MIDI input watching, behind the activity LED on the main screen.

python-rtmidi delivers messages on its own thread, so nothing here touches the
UI. The listener only records *when* a message last arrived; the UI polls that
timestamp on its own timer and lights the LED. python-rtmidi is the direct
counterpart of Rust's `midir`, should this ever be ported.
"""

import time

import rtmidi

# Status bytes from 0xF8 up are System Real-Time: MIDI clock, active sensing and
# friends. Controllers stream those continuously whether or not anyone is
# playing, so counting them as activity would leave the LED permanently lit and
# telling you nothing.
REALTIME_STATUS_FLOOR = 0xF8

# How long the LED stays lit after a message. Long enough to catch the eye on a
# single note, short enough that playing reads as a flicker rather than a hold.
ACTIVITY_WINDOW = 0.12


def is_activity(message):
    """Whether a raw MIDI message means someone is actually playing."""
    if not message:
        return False
    return message[0] < REALTIME_STATUS_FLOOR


def list_devices():
    """Names of the MIDI input ports currently connected."""
    midi_in = rtmidi.MidiIn()
    try:
        return list(midi_in.get_ports())
    finally:
        midi_in.delete()


def choose_device(saved, available):
    """Pick the port to listen on.

    The saved device when it is plugged in, otherwise the first one that is — a
    device named in settings.json may simply not be connected yet. None when
    nothing is connected at all.

    Free of rtmidi on purpose, so the fallback is testable without hardware.
    """
    if saved and saved in available:
        return saved
    return available[0] if available else None


class MidiListener:
    """Watches one MIDI input port and records when it last saw real activity."""

    def __init__(self):
        self._midi_in = None
        self.device = None
        # Far enough in the past that the LED starts dark.
        self.last_message_at = 0.0

    def open(self, device_name):
        """Listen on the named port. Returns whether it opened.

        Closes any port already open, so this doubles as "switch to". Whether a
        second program can open the same device at the same time is up to the
        driver — the Portable G-1 allows it, others do not.
        """
        self.close()
        if not device_name:
            return False

        midi_in = rtmidi.MidiIn()
        try:
            ports = list(midi_in.get_ports())
            if device_name not in ports:
                midi_in.delete()
                return False

            midi_in.open_port(ports.index(device_name))
            # Belt and braces: rtmidi already drops these by default, but the
            # LED depends on it, so say so rather than inherit it.
            midi_in.ignore_types(sysex=True, timing=True, active_sense=True)
            midi_in.set_callback(self._on_message)
        except Exception as e:
            print(f"Could not open MIDI device '{device_name}': {e}")
            midi_in.delete()
            return False

        self._midi_in = midi_in
        self.device = device_name
        return True

    def _on_message(self, event, data=None):
        """rtmidi callback — runs on rtmidi's own thread, so it only writes a
        timestamp and never touches the UI."""
        message, _delta = event
        if is_activity(message):
            self.last_message_at = time.monotonic()

    def is_active(self, now=None):
        """Whether a message arrived recently enough to light the LED."""
        if not self.last_message_at:
            return False
        now = time.monotonic() if now is None else now
        return (now - self.last_message_at) < ACTIVITY_WINDOW

    def close(self):
        """Release the port. Safe to call when nothing is open."""
        if self._midi_in is not None:
            try:
                self._midi_in.close_port()
                self._midi_in.delete()
            except Exception:
                pass  # Already gone; nothing useful left to do.
            self._midi_in = None
        self.device = None
