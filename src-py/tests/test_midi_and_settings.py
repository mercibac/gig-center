"""Tests for the parts of the MIDI LED and shutdown toggle that can be checked
without a controller plugged in or the PC actually powering off.

Run with:  uv run --with pytest pytest src-py/tests
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

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
        show_settings = True

    app = object.__new__(main.GigControlApp)
    app.app = FakeUi()
    app.refresh_midi_devices = lambda: None  # no hardware under test
    app.settings = {
        "phone_name": "old phone",
        "hotspot_name": "old hotspot",
        "osc_file_path": "",
        "midi_device": "MPK mini 0",
        "shutdown_after_clear": True,
    }

    app.update_settings("New Phone", "New Hotspot", "C:/preset.json")

    saved = json.loads(config.read_text())
    assert saved["phone_name"] == "New Phone"
    assert saved["midi_device"] == "MPK mini 0"
    assert saved["shutdown_after_clear"] is True, "the toggle was lost on save"
    assert app.app.show_settings is False  # modal closes
