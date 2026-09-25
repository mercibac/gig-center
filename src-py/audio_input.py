"""Audio output watching, behind the activity LED at the bottom of the screen.

Reads the endpoint's peak meter rather than capturing audio: no stream, no new
dependency, negligible cost. `utils.set_volume` already performs the same COM
dance for the volume control.

Polling runs on its own thread so the COM calls never touch the UI thread, and
it records a timestamp the UI polls on its own timer — the same shape
`midi_input` uses for MIDI. Port of src-rs/audio_input.rs.
"""

import threading
import time
from ctypes import HRESULT, POINTER, c_float

import comtypes
from comtypes import COMMETHOD, GUID, IUnknown
from pycaw.constants import DEVICE_STATE, EDataFlow
from pycaw.pycaw import AudioUtilities

# Idle endpoints read exactly 0.0 — measured across all six outputs on this
# machine, ASIO ones included — while real playback read 0.03 and up. So the
# bar only has to clear true silence, and sitting this low keeps quiet passages
# from dropping the LED.
SIGNAL_THRESHOLD = 0.002

# How long the LED stays lit after signal. Matches the MIDI LED so the two
# behave alike, and bridges the gaps between peaks within a note.
ACTIVITY_WINDOW = 0.12

# How often the watcher thread reads the meter.
POLL_INTERVAL = 0.03


class IAudioMeterInformation(IUnknown):
    """The one WASAPI interface pycaw does not ship.

    Only GetPeakValue is declared, because it is first in the vtable and the
    only method needed; anything past it would require declaring the ones
    before it in order.
    """

    _iid_ = GUID("{C02216F6-8C67-4B5B-9D00-D008E73E0064}")
    _methods_ = (
        COMMETHOD([], HRESULT, "GetPeakValue", (["out"], POINTER(c_float), "pfPeak")),
    )


def is_signal(peak):
    """Whether a peak reading counts as audio playing."""
    return peak > SIGNAL_THRESHOLD


def _render_devices():
    """The active render endpoints, as pycaw device objects."""
    return AudioUtilities.GetAllDevices(
        EDataFlow.eRender.value, DEVICE_STATE.ACTIVE.value
    )


def list_devices():
    """Friendly names of the audio outputs currently active."""
    try:
        return [d.FriendlyName for d in _render_devices() if d.FriendlyName]
    except Exception as e:
        print(f"Could not list audio outputs: {e}")
        return []


def choose_device(saved, available):
    """Pick the output to watch.

    The saved one when it is present, otherwise the first available — an output
    named in settings.json may simply be switched off today. None when the
    machine reports no outputs at all.

    Free of COM on purpose, so the fallback is testable without hardware.
    """
    if saved and saved in available:
        return saved
    return available[0] if available else None


def _meter_for(device_name):
    """Find the peak meter for a named output, or None.

    Must run on a COM-initialised thread. `_dev.Activate` is the same call
    pycaw's own `EndpointVolume` property relies on.
    """
    for device in _render_devices():
        if device.FriendlyName == device_name:
            iface = device._dev.Activate(
                IAudioMeterInformation._iid_, comtypes.CLSCTX_ALL, None
            )
            return iface.QueryInterface(IAudioMeterInformation)
    return None


class AudioMeter:
    """Watches one audio output and records when it last carried signal."""

    def __init__(self):
        self.device = None
        # Far enough in the past that the LED starts dark.
        self.last_signal_at = 0.0
        self._stop = None

    def open(self, device_name):
        """Watch the named output. Returns whether a watcher started.

        Stops any watcher already running, so this doubles as "switch to" —
        which is what lets a saved change take effect without a restart.
        """
        self.close()
        if not device_name:
            return False

        stop = threading.Event()
        self._stop = stop
        self.device = device_name
        threading.Thread(
            target=self._watch, args=(device_name, stop), daemon=True
        ).start()
        return True

    def _watch(self, device_name, stop):
        """Poll the meter until asked to stop.

        Owns its COM apartment and does its own lookup: COM pointers do not
        cross apartments safely, so only the device *name* is handed over.
        """
        comtypes.CoInitialize()
        try:
            meter = _meter_for(device_name)
            if meter is None:
                print(f"Could not watch audio output '{device_name}'.")
                return
            while not stop.is_set():
                try:
                    if is_signal(meter.GetPeakValue()):
                        self.last_signal_at = time.monotonic()
                except Exception:
                    # The endpoint went away mid-poll — unplugged, or the
                    # driver reloaded. Stop, rather than log every 30ms.
                    return
                stop.wait(POLL_INTERVAL)
        except Exception as e:
            print(f"Audio watch on '{device_name}' stopped: {e}")
        finally:
            comtypes.CoUninitialize()

    def is_active(self, now=None):
        """Whether signal arrived recently enough to light the LED."""
        if not self.last_signal_at:
            return False
        now = time.monotonic() if now is None else now
        return (now - self.last_signal_at) < ACTIVITY_WINDOW

    def close(self):
        """Stop watching. Safe to call when nothing is running.

        The thread notices within one poll interval and releases its own COM
        state, so repeated switching cannot pile threads up.
        """
        if self._stop is not None:
            self._stop.set()
        self._stop = None
        self.device = None
