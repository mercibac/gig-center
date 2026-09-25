"""Tests for the parts of the MIDI LED and shutdown toggle that can be checked
without a controller plugged in or the PC actually powering off.

Run with:  uv run --with pytest pytest src-py/tests
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import audio_input  # noqa: E402
import midi_input  # noqa: E402


# ---------------------------------------------------------
# Device fallback — the "if not available yet" rule
# ---------------------------------------------------------
def test_saved_device_is_used_when_connected():
    assert (
        midi_input.choose_device("MPK mini 0", ["Launchkey 1", "MPK mini 0"])
        == "MPK mini 0"
    )


def test_falls_back_to_first_available_when_saved_device_is_absent():
    # The gig rig is set up, but a different controller is plugged in today.
    assert midi_input.choose_device("MPK mini 0", ["Launchkey 1"]) == "Launchkey 1"


def test_falls_back_when_nothing_was_ever_saved():
    assert midi_input.choose_device("", ["Launchkey 1"]) == "Launchkey 1"


def test_no_device_when_nothing_is_connected():
    assert midi_input.choose_device("MPK mini 0", []) is None


# ---------------------------------------------------------
# Activity filter — what actually lights the LED
# ---------------------------------------------------------
def test_note_on_counts_as_activity():
    assert midi_input.is_activity([0x90, 60, 100]) is True


def test_control_change_counts_as_activity():
    assert midi_input.is_activity([0xB0, 7, 127]) is True


def test_midi_clock_does_not_count():
    # Sent ~24x per beat forever; counting it would pin the LED on.
    assert midi_input.is_activity([0xF8]) is False


def test_active_sensing_does_not_count():
    # Streamed constantly by many controllers even when untouched.
    assert midi_input.is_activity([0xFE]) is False


def test_empty_message_does_not_count():
    assert midi_input.is_activity([]) is False


# ---------------------------------------------------------
# LED decay
# ---------------------------------------------------------
def test_led_starts_dark():
    assert midi_input.MidiListener().is_active() is False


def test_led_lights_then_decays():
    listener = midi_input.MidiListener()
    listener._on_message(([0x90, 60, 100], 0.0))

    at = listener.last_message_at
    assert listener.is_active(now=at) is True
    assert listener.is_active(now=at + midi_input.ACTIVITY_WINDOW + 0.01) is False


def test_clock_message_never_lights_the_led():
    listener = midi_input.MidiListener()
    listener._on_message(([0xF8], 0.0))
    assert listener.is_active() is False


# ---------------------------------------------------------
# Settings round-trip — the regression a wholesale rebuild would cause
# ---------------------------------------------------------
def test_saving_settings_keeps_the_new_keys(tmp_path, monkeypatch):
    """Saving from the settings modal must not drop the MIDI device or the
    shutdown flag, which live outside that form."""
    import main  # noqa: E402

    config = tmp_path / "settings.json"
    monkeypatch.setattr(main, "CONFIG_FILE", str(config))

    settings = {
        "phone_name": "Phone",
        "hotspot_name": "Hotspot",
        "osc_file_path": "C:/preset.json",
        "midi_device": "MPK mini 0",
        "shutdown_after_clear": True,
    }
    main.save_settings(settings)

    reloaded = json.loads(config.read_text())
    assert reloaded["midi_device"] == "MPK mini 0"
    assert reloaded["shutdown_after_clear"] is True


def test_defaults_carry_the_new_keys():
    import main  # noqa: E402

    assert main.DEFAULT_CONFIG["midi_device"] == ""
    assert main.DEFAULT_CONFIG["shutdown_after_clear"] is False


# ---------------------------------------------------------
# Shutdown ordering — every destructive step is stubbed, so this exercises the
# real main() without killing anything or powering the machine off
# ---------------------------------------------------------
def _stub_clear_deck(monkeypatch, calls):
    import gig_close

    for name in (
        "switch_sleep",
        "switch_defender",
        "set_volume",
        "force_kill",
        "clear_the_deck",
        "shutdown_pc",
    ):
        monkeypatch.setattr(
            gig_close, name, lambda *a, _n=name, **kw: calls.append(_n)
        )
    return gig_close


def test_shutdown_runs_after_every_other_step(monkeypatch):
    calls = []
    gig_close = _stub_clear_deck(monkeypatch, calls)

    gig_close.main(shutdown_after=True)

    assert calls[-1] == "shutdown_pc", f"shutdown was not last: {calls}"
    assert "force_kill" in calls and "clear_the_deck" in calls
    assert calls.index("clear_the_deck") < calls.index("shutdown_pc")


def test_no_shutdown_when_the_toggle_is_off(monkeypatch):
    calls = []
    gig_close = _stub_clear_deck(monkeypatch, calls)

    gig_close.main(shutdown_after=False)

    assert "shutdown_pc" not in calls
    assert "clear_the_deck" in calls  # the teardown still ran


def test_shutdown_waits_then_issues_the_command(monkeypatch):
    import gig_close

    slept = []
    ran = []
    monkeypatch.setattr(gig_close.time, "sleep", lambda s: slept.append(s))
    monkeypatch.setattr(
        gig_close.subprocess, "run", lambda cmd, **kw: ran.append(cmd)
    )

    gig_close.shutdown_pc()

    assert slept == [2], "the 2 second gap before shutdown is missing"
    assert ran == [["shutdown", "/s", "/t", "0"]]


def test_main_defaults_to_not_shutting_down(monkeypatch):
    calls = []
    gig_close = _stub_clear_deck(monkeypatch, calls)

    gig_close.main()  # standalone `python gig_close.py` must stay safe

    assert "shutdown_pc" not in calls


def test_saving_the_modal_preserves_settings_it_does_not_show(tmp_path, monkeypatch):
    """The shutdown toggle lives on the main screen and the MIDI device is only
    a dropdown, so a Save must merge rather than rebuild the settings dict."""
    import main

    config = tmp_path / "settings.json"
    monkeypatch.setattr(main, "CONFIG_FILE", str(config))

    class FakeUi:
        midi_device = "MPK mini 0"
        audio_device = "Example Output (Generic Audio)"
        show_settings = True

    app = object.__new__(main.GigControlApp)
    app.app = FakeUi()
    app.refresh_midi_devices = lambda: None  # no hardware under test
    app.refresh_audio_devices = lambda: None
    app.settings = {
        "phone_name": "old phone",
        "hotspot_name": "old hotspot",
        "osc_file_path": "",
        "midi_device": "MPK mini 0",
        "shutdown_after_clear": True,
        "manage_defender": False,
    }

    app.update_settings("New Phone", "New Hotspot", "C:/preset.json")

    saved = json.loads(config.read_text())
    assert saved["phone_name"] == "New Phone"
    assert saved["midi_device"] == "MPK mini 0"
    assert saved["shutdown_after_clear"] is True, "the toggle was lost on save"
    assert saved["manage_defender"] is False, "the Defender button was lost on save"
    assert app.app.show_settings is False  # modal closes


# ---------------------------------------------------------
# MIDI ownership — the app must get out of Gig Performer's way
# ---------------------------------------------------------
class _FakeListener:
    """Stands in for MidiListener: records opens and closes, no hardware."""

    def __init__(self, device=None):
        self.device = device
        self.opened = []
        self.closed = 0

    def open(self, name):
        self.opened.append(name)
        self.device = name
        return True

    def close(self):
        self.closed += 1
        self.device = None


class _FakeUi:
    midi_active = True
    midi_device = ""
    midi_devices = None
    launch_text = ""
    clear_text = ""
    buttons_enabled = False


def _app(monkeypatch, listener, devices=("MPK mini 0",), released=False):
    import main

    monkeypatch.setattr(main.midi_input, "list_devices", lambda: list(devices))
    app = object.__new__(main.GigControlApp)
    app.app = _FakeUi()
    app.midi = listener
    app.midi_released = released
    app.settings = {"midi_device": "MPK mini 0"}
    return app


def test_release_hands_the_port_over_and_darkens_the_led(monkeypatch):
    listener = _FakeListener(device="MPK mini 0")
    app = _app(monkeypatch, listener)

    app.release_midi()

    assert listener.closed == 1
    assert app.midi_released is True
    assert app.app.midi_active is False


def test_refresh_does_not_steal_the_port_back_while_released(monkeypatch):
    """Opening the settings modal mid-gig must not reopen the device."""
    listener = _FakeListener()
    app = _app(monkeypatch, listener, released=True)

    app.refresh_midi_devices()

    assert listener.opened == [], "settings grabbed MIDI back from Gig Performer"
    assert app.app.midi_device == "MPK mini 0"  # dropdown still populated


def test_refresh_opens_normally_when_not_released(monkeypatch):
    listener = _FakeListener()
    app = _app(monkeypatch, listener)

    app.refresh_midi_devices()

    assert listener.opened == ["MPK mini 0"]


def test_nothing_takes_the_port_back_once_released(monkeypatch):
    """Released is for the rest of the session: finishing CLEAR DECK, or any
    other unlock, must leave the device with Gig Performer. Only a restart
    starts listening again."""
    listener = _FakeListener()
    app = _app(monkeypatch, listener, released=True)

    app.unlock_buttons()

    assert app.midi_released is True
    assert listener.opened == []


def test_release_survives_reopening_settings(monkeypatch):
    """The one path that could quietly reclaim the device."""
    listener = _FakeListener()
    app = _app(monkeypatch, listener, released=True)

    app.refresh_midi_devices()   # what opening the settings modal does
    app.unlock_buttons()         # what finishing CLEAR DECK does

    assert listener.opened == []
    assert app.midi_released is True


# ---------------------------------------------------------
# ctypes hygiene — window_setup must keep its argtypes to itself
# ---------------------------------------------------------
def test_window_setup_leaves_the_shared_user32_alone():
    """ctypes.windll is process-wide, so configuring argtypes on it rewrites
    those signatures for every library in the process.

    Doing that to GetWindowRect broke pygetwindow, which passes a pointer to its
    own RECT class: the exception fired inside an EnumWindows callback, aborted
    the walk, and left CLEAR DECK iterating an empty window list while still
    reporting success.
    """
    import ctypes

    import window_setup  # noqa: F401  -- sets its argtypes at import time

    assert ctypes.windll.user32.GetWindowRect.argtypes is None, (
        "window_setup configured the shared ctypes.windll.user32; "
        "it must use its own ctypes.WinDLL handle"
    )


# ---------------------------------------------------------
# Defender button — it must actually skip the step, not just look off
# ---------------------------------------------------------
def _stub_launcher(monkeypatch, calls):
    """Stub every destructive step in gig_launcher.main.

    ensure_correct_network returns False so main stops right after the Defender
    step, which is all this needs to observe — nothing launches or moves windows.
    """
    import gig_launcher

    monkeypatch.setattr(
        gig_launcher, "load_config", lambda: {"hotspot_name": "hotspot"}
    )
    for name in ("switch_sleep", "switch_defender", "set_volume"):
        monkeypatch.setattr(
            gig_launcher, name, lambda *a, _n=name, **kw: calls.append(_n)
        )
    monkeypatch.setattr(
        gig_launcher.connection, "ensure_correct_network", lambda target: False
    )
    return gig_launcher


def test_clear_deck_skips_defender_when_the_button_is_off(monkeypatch):
    calls = []
    gig_close = _stub_clear_deck(monkeypatch, calls)

    gig_close.main(manage_defender=False)

    assert "switch_defender" not in calls
    # the rest of the teardown still runs
    assert "switch_sleep" in calls and "clear_the_deck" in calls


def test_clear_deck_touches_defender_when_the_button_is_on(monkeypatch):
    calls = []
    gig_close = _stub_clear_deck(monkeypatch, calls)

    gig_close.main(manage_defender=True)

    assert "switch_defender" in calls


def test_launch_skips_defender_when_the_button_is_off(monkeypatch):
    calls = []
    gig_launcher = _stub_launcher(monkeypatch, calls)

    gig_launcher.main(manage_defender=False)

    assert "switch_defender" not in calls
    assert "switch_sleep" in calls and "set_volume" in calls


def test_launch_touches_defender_when_the_button_is_on(monkeypatch):
    calls = []
    gig_launcher = _stub_launcher(monkeypatch, calls)

    gig_launcher.main(manage_defender=True)

    assert "switch_defender" in calls


def test_both_entry_points_default_to_managing_defender(monkeypatch):
    """Running either module standalone must behave as it always has."""
    calls = []
    gig_close = _stub_clear_deck(monkeypatch, calls)
    gig_close.main()
    assert "switch_defender" in calls

    calls.clear()
    gig_launcher = _stub_launcher(monkeypatch, calls)
    gig_launcher.main()
    assert "switch_defender" in calls


def test_defaults_carry_the_defender_flag():
    import main

    assert main.DEFAULT_CONFIG["manage_defender"] is True


# ---------------------------------------------------------
# Saving with the controller unplugged must not forget it
# ---------------------------------------------------------
def test_saving_while_unplugged_keeps_the_remembered_device(tmp_path, monkeypatch):
    """The dropdown reads empty when no controller is connected, so a Save of
    unrelated settings used to overwrite the stored device name with "".

    This bit for real: changing the hotspot name with the controller detached
    silently lost 'Portable G-1 0' from settings.json.
    """
    import main

    config = tmp_path / "settings.json"
    monkeypatch.setattr(main, "CONFIG_FILE", str(config))

    class UnpluggedUi:
        midi_device = ""  # nothing connected, so the ComboBox is empty
        audio_device = ""
        show_settings = True

    app = object.__new__(main.GigControlApp)
    app.app = UnpluggedUi()
    app.refresh_midi_devices = lambda: None
    app.refresh_audio_devices = lambda: None
    app.settings = {
        "phone_name": "Phone",
        "hotspot_name": "old hotspot",
        "osc_file_path": "",
        "midi_device": "Portable G-1 0",
        "shutdown_after_clear": False,
        "manage_defender": True,
    }

    app.update_settings("Phone", "NEW HOTSPOT", "")

    saved = json.loads(config.read_text())
    assert saved["hotspot_name"] == "NEW HOTSPOT"
    assert saved["midi_device"] == "Portable G-1 0", (
        "saving with the controller unplugged wiped the remembered device"
    )


def test_selecting_a_device_still_overwrites_the_stored_one(tmp_path, monkeypatch):
    """The fallback must not pin the old value: picking a different device in
    the dropdown has to win."""
    import main

    config = tmp_path / "settings.json"
    monkeypatch.setattr(main, "CONFIG_FILE", str(config))

    class PluggedUi:
        midi_device = "Launchkey 49 MIDI"
        audio_device = "Example Output (Generic Audio)"
        show_settings = True

    app = object.__new__(main.GigControlApp)
    app.app = PluggedUi()
    app.refresh_midi_devices = lambda: None
    app.refresh_audio_devices = lambda: None
    app.settings = {
        "phone_name": "Phone",
        "hotspot_name": "Hotspot",
        "osc_file_path": "",
        "midi_device": "Portable G-1 0",
    }

    app.update_settings("Phone", "Hotspot", "")

    saved = json.loads(config.read_text())
    assert saved["midi_device"] == "Launchkey 49 MIDI"


# ---------------------------------------------------------
# Audio output fallback — same rule as MIDI, different hardware
# ---------------------------------------------------------
def test_saved_output_is_used_when_present():
    available = ["Casque (Realtek(R) Audio)", "Speakers 01 (ASIOVADPRO Driver)"]
    assert (
        audio_input.choose_device("Speakers 01 (ASIOVADPRO Driver)", available)
        == "Speakers 01 (ASIOVADPRO Driver)"
    )


def test_audio_falls_back_to_first_available():
    # The saved output is switched off today, so meter whatever is live.
    assert audio_input.choose_device("Speakers 01", ["Casque"]) == "Casque"


def test_audio_falls_back_when_nothing_was_ever_saved():
    assert audio_input.choose_device("", ["Casque"]) == "Casque"


def test_no_output_when_none_are_active():
    assert audio_input.choose_device("Casque", []) is None


# ---------------------------------------------------------
# Signal threshold — measured against this machine's endpoints
# ---------------------------------------------------------
def test_silence_does_not_count_as_signal():
    # Every idle endpoint, ASIO ones included, reported exactly 0.0.
    assert audio_input.is_signal(0.0) is False


def test_real_playback_counts_as_signal():
    # Sampled while audio played: 0.030809.
    assert audio_input.is_signal(0.030809) is True


def test_audio_led_starts_dark():
    assert audio_input.AudioMeter().is_active() is False


def test_audio_led_lights_then_decays():
    meter = audio_input.AudioMeter()
    meter.last_signal_at = 100.0

    assert meter.is_active(now=100.0) is True
    assert meter.is_active(now=100.0 + audio_input.ACTIVITY_WINDOW + 0.01) is False


# ---------------------------------------------------------
# Saving the output must both persist it and re-point the meter
# ---------------------------------------------------------
class _FakeMeter:
    """Stands in for AudioMeter: records opens and closes, no COM, no threads."""

    def __init__(self, device=None):
        self.device = device
        self.opened = []
        self.closed = 0

    def open(self, name):
        self.opened.append(name)
        self.device = name
        return True

    def close(self):
        self.closed += 1
        self.device = None


def _save_app(monkeypatch, tmp_path, ui, stored, outputs=("Old Out", "New Out")):
    import main

    config = tmp_path / "settings.json"
    monkeypatch.setattr(main, "CONFIG_FILE", str(config))
    monkeypatch.setattr(main.audio_input, "list_devices", lambda: list(outputs))

    app = object.__new__(main.GigControlApp)
    app.app = ui
    app.refresh_midi_devices = lambda: None  # MIDI is not what is under test
    app.audio = _FakeMeter(device="Old Out")
    app.settings = stored
    return app, config


def test_saving_rewatches_the_newly_chosen_output(tmp_path, monkeypatch):
    """The whole point of the save-time refresh: the meter must follow."""

    class Ui:
        midi_device = ""
        audio_device = "New Out"
        audio_devices = None
        show_settings = True

    app, config = _save_app(
        monkeypatch,
        tmp_path,
        Ui(),
        {
            "phone_name": "Phone",
            "hotspot_name": "Hotspot",
            "osc_file_path": "",
            "midi_device": "",
            "audio_device": "Old Out",
        },
    )

    app.update_settings("Phone", "Hotspot", "")

    assert json.loads(config.read_text())["audio_device"] == "New Out"
    assert app.audio.opened == ["New Out"], "the meter kept watching the old output"


def test_saving_with_an_empty_audio_dropdown_keeps_the_stored_output(
    tmp_path, monkeypatch
):
    """The dropdown reads empty before enumeration finishes; taking that at face
    value would throw the remembered output away."""

    class Ui:
        midi_device = ""
        audio_device = ""  # nothing selected yet
        audio_devices = None
        show_settings = True

    app, config = _save_app(
        monkeypatch,
        tmp_path,
        Ui(),
        {
            "phone_name": "Phone",
            "hotspot_name": "Hotspot",
            "osc_file_path": "",
            "midi_device": "",
            "audio_device": "Old Out",
        },
    )

    app.update_settings("Phone", "Hotspot", "")

    assert json.loads(config.read_text())["audio_device"] == "Old Out"


def test_audio_default_carries_the_new_key():
    import main

    assert main.DEFAULT_CONFIG["audio_device"] == ""


# ---------------------------------------------------------
# Startup gate — the UI stays veiled until both slow parts finish
# ---------------------------------------------------------
class _GateUi:
    ready = False
    console_text = ""


def _gate_app(audio_ready, warm_done):
    import main
    import threading as _t

    app = object.__new__(main.GigControlApp)
    app.app = _GateUi()
    app._audio_ready = audio_ready
    app._ready_announced = False
    app._warm_done = _t.Event()
    if warm_done:
        app._warm_done.set()
    return app


def test_stays_veiled_until_audio_enumeration_finishes():
    app = _gate_app(audio_ready=False, warm_done=True)

    app.poll_console()

    assert app.app.ready is False, "veil lifted before the outputs were listed"


def test_stays_veiled_until_the_heavy_import_finishes():
    app = _gate_app(audio_ready=True, warm_done=False)

    app.poll_console()

    assert app.app.ready is False, "veil lifted before the ~1s import finished"


def test_lifts_the_veil_once_both_are_done():
    app = _gate_app(audio_ready=True, warm_done=True)

    app.poll_console()

    assert app.app.ready is True


def test_the_veil_is_lifted_only_once():
    """Re-announcing every 100ms would restart the Ready toast forever."""
    app = _gate_app(audio_ready=True, warm_done=True)

    app.poll_console()
    app.app.ready = False  # as if the UI had moved on
    app.poll_console()

    assert app.app.ready is False, "readiness was announced more than once"


def test_a_failed_warm_up_still_releases_the_gate(monkeypatch):
    """A broken pre-import must not leave the app stuck behind the veil."""
    app = _gate_app(audio_ready=True, warm_done=False)

    def explode():
        raise ImportError("no cv2 for you")

    monkeypatch.setitem(sys.modules, "gig_launcher", None)
    monkeypatch.setattr(
        "builtins.__import__", lambda *a, **kw: explode(), raising=True
    )
    try:
        app._warm_up()
    finally:
        monkeypatch.undo()

    assert app._warm_done.is_set(), "the gate would never open"

    app.poll_console()
    assert app.app.ready is True
