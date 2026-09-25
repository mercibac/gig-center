import comtypes
import slint
import threading
import queue
import os
import sys
import json
import datetime

# Lightweight config import
from utils import CONFIG_FILE, bundled_settings, ensure_settings_file, resource_path
import audio_input
import midi_input
import window_setup


# --- Configuration Manager ---
DEFAULT_CONFIG = {
    "phone_name": "SMART PHONE",
    "hotspot_name": "SHARED ACCESS POINT",
    "osc_file_path": "",
    "midi_device": "",
    "audio_device": "",
    "shutdown_after_clear": False,
    "manage_defender": True,
}


def load_settings():
    # A packaged build ships settings.json inside the bundle; copy it out before
    # looking, so a fresh install starts from those values instead of the
    # placeholders below.
    ensure_settings_file()

    config_path = CONFIG_FILE
    if not os.path.exists(config_path):
        return _restore_defaults()
    try:
        with open(config_path, "r") as f:
            return json.load(f)
    except Exception as e:
        print(f"Warning: settings.json is corrupted. Repairing... ({e})")
        return _restore_defaults()


def _restore_defaults():
    """Write and return the shipped settings, or the placeholders above when the
    build has none bundled."""
    bundled = bundled_settings()
    settings = DEFAULT_CONFIG if bundled is None else bundled
    save_settings(settings)
    return settings


def save_settings(data):
    config_path = CONFIG_FILE
    with open(config_path, "w") as f:
        json.dump(data, f, indent=4)


# ==========================================
# THREAD-SAFE CONSOLE REDIRECTOR (Optimized)
# ==========================================
console_queue = queue.Queue()


# ==========================================
# UI COMPILATION
# ==========================================
_components = None


def load_ui():
    """Compile ui/app.slint once.

    The splash and the main window both come from it, and compiling it twice
    would pay for the whole UI twice on the startup path.
    """
    global _components
    if _components is None:
        _components = slint.load_file(resource_path("ui/app.slint"))
    return _components


class ConsoleRedirector:
    def write(self, string):
        if string:  # Only queue non-empty strings
            console_queue.put(string)

    def flush(self):
        pass

    def close(self):
        pass


# ==========================================
# MAIN APPLICATION CLASS
# ==========================================
class GigControlApp:
    def __init__(self):
        self.settings = load_settings()

        self.app = load_ui().MainWindow()

        # Load initial settings into UI properties
        self.app.phone_name = self.settings.get("phone_name", "")
        self.app.hotspot_name = self.settings.get("hotspot_name", "")
        self.app.osc_path = self.settings.get("osc_file_path", "")
        self.app.shutdown_after_clear = bool(
            self.settings.get("shutdown_after_clear", False)
        )
        self.app.manage_defender = bool(self.settings.get("manage_defender", True))

        self.app.console_text = "System initialized. Awaiting command...\n"

        sys.stdout = ConsoleRedirector()
        sys.stderr = ConsoleRedirector()

        # After the redirector, so "Listening for MIDI on ..." shows up in the
        # on-screen console instead of a terminal nobody is watching.
        self.midi = midi_input.MidiListener()
        # Set once LAUNCH GIG RIG hands the input over; never cleared.
        self.midi_released = False
        # Devices are not listed here. Listing is the slow part of starting up
        # and runs on a worker thread instead, so the app can tell whether it is
        # ready before deciding to show a splash. run_with_splash applies the
        # result through apply_midi_devices / apply_audio_devices.

        # Enumerating audio endpoints measures ~180ms, because pycaw builds a
        # property store per device. It is not done here: the splash sequence
        # runs it as its own announced step, so the user watches it happen
        # instead of waiting on it.
        self.audio = audio_input.AudioMeter()

        # Optimized timer: 100ms instead of 50ms reduces overhead
        self.console_timer = slint.Timer()
        self.console_timer.start(
            slint.TimerMode.Repeated,
            datetime.timedelta(milliseconds=100),  # Reduced from 50ms to 100ms
            self.poll_console,
        )

        # Faster than the console timer: the LED is a flicker, not a readout.
        # rtmidi's callback runs on its own thread, so polling from a Slint
        # timer is what keeps the UI property on the UI thread.
        self.midi_timer = slint.Timer()
        self.midi_timer.start(
            slint.TimerMode.Repeated,
            datetime.timedelta(milliseconds=40),
            self.poll_midi,
        )

        # Centring and the title-bar icon are not applied here: the window is
        # not shown until the splash hands over, and run_with_splash dresses it
        # at that point. Doing it here as well would dress the splash twice.

        # Connect UI Callbacks to Python functions
        self.app.launch_clicked = self.run_launch_script
        self.app.clear_clicked = self.run_clear_script
        self.app.save_clicked = self.update_settings
        self.app.shutdown_toggled = self.update_shutdown_option
        self.app.defender_toggled = self.update_defender_option
        self.app.settings_opened = self.refresh_devices

    def poll_console(self):
        texts = []
        while not console_queue.empty():
            msg = console_queue.get_nowait()
            if msg == "__UNLOCK__":
                self.unlock_buttons()
                continue
            texts.append(msg)

        if texts:
            self.app.console_text += "".join(texts)

    def poll_midi(self):
        # One timer drives both LEDs: they run at the same rate and neither
        # does more than read a timestamp.
        self.app.midi_active = self.midi.is_active()
        self.app.audio_active = self.audio.is_active()

    # --- MIDI Logic ---
    def refresh_midi_devices(self, announce=False):
        """Re-read the connected devices and (re)open the one to listen on.

        Runs at startup and every time the settings modal opens, so a controller
        plugged in mid-session shows up without a restart. Reports only when
        something changed, so reopening settings does not spam the console —
        except at startup, where `announce` forces one line so the state is
        never a mystery.
        """
        self.apply_midi_devices(midi_input.list_devices(), announce)

    def apply_midi_devices(self, devices, announce=False):
        """The half of the above that touches the UI, given a list in hand.

        Split out so startup can enumerate on a worker thread and still end up
        here, on the UI thread, running what the settings and save paths run.
        """
        self.app.midi_devices = slint.ListModel(devices)

        wanted = midi_input.choose_device(self.settings.get("midi_device", ""), devices)
        self.app.midi_device = wanted or ""

        # Listing ports does not open them, so the dropdown above stays current
        # even mid-gig. Reopening one would put this app back on the device
        # behind the user's back, which is what release_midi exists to prevent.
        if self.midi_released:
            return

        if wanted != self.midi.device or announce:
            if wanted:
                if self.midi.open(wanted):
                    print(f"Listening for MIDI on '{wanted}'.")
            else:
                self.midi.close()
                print("No MIDI device connected.")

    def refresh_devices(self):
        """Re-enumerate both kinds of device, for the settings modal.

        Opening settings is the moment to notice a controller or an output that
        appeared mid-session, so both lists are rebuilt together.
        """
        self.refresh_midi_devices()
        self.refresh_audio_devices()

    def refresh_audio_devices(self, announce=False):
        """Re-read the audio outputs and (re)watch the one to meter.

        Mirrors refresh_midi_devices, minus the release rule: nothing competes
        with this app for a peak meter, so there is no reason to ever stop
        watching.
        """
        self.apply_audio_devices(audio_input.list_devices(), announce)

    def apply_audio_devices(self, devices, announce=False):
        """The half of the above that touches the UI, given a list in hand."""
        self.app.audio_devices = slint.ListModel(devices)

        wanted = audio_input.choose_device(
            self.settings.get("audio_device", ""), devices
        )
        self.app.audio_device = wanted or ""

        if wanted != self.audio.device or announce:
            if wanted:
                if self.audio.open(wanted):
                    print(f"Watching audio on '{wanted}'.")
            else:
                self.audio.close()
                print("No audio output found.")

    def release_midi(self):
        """Hand the MIDI input to Gig Performer.

        Measured on a Portable G-1: the driver is multi-client, so holding the
        port does not in fact lock Gig Performer out. This closes it anyway —
        the app has no reason to sit on the device during a gig, and a driver
        that *is* single-client would otherwise block the one program that
        matters.

        The handover lasts for the rest of the session: nothing takes the
        device back, not CLEAR DECK and not reopening the settings modal. Only
        restarting the app starts listening again. That way the running app can
        never surprise Gig Performer by reclaiming the input mid-gig.
        """
        self.midi_released = True
        self.midi.close()
        self.app.midi_active = False
        print("MIDI released for the rest of the session — restart to listen again.")

    # --- Button Logic ---
    def lock_buttons(self, status_text):
        self.app.launch_text = status_text
        self.app.clear_text = "WORKING..."
        self.app.buttons_enabled = False

    def unlock_buttons(self):
        self.app.launch_text = "LAUNCH GIG RIG"
        self.app.clear_text = "CLEAR DECK"
        self.app.buttons_enabled = True
        print("\n--- SYSTEM READY ---")

    def run_launch_script(self):
        # Before anything launches, so the port is free the moment Gig
        # Performer looks for it.
        self.release_midi()
        self.lock_buttons("INITIALIZING...")
        self.app.console_text = ""
        # Lazy import of heavy module
        threading.Thread(
            target=self.execute_module, args=(self._launch_gig,), daemon=True
        ).start()

    def _launch_gig(self):
        """Lazy load and execute gig_launcher module"""
        import gig_launcher  # Heavy module imported only when needed

        gig_launcher.main(manage_defender=bool(self.settings.get("manage_defender", True)))

    def run_clear_script(self):
        self.lock_buttons("CLEARING...")
        self.app.console_text = ""
        # Lazy import of heavy module
        threading.Thread(
            target=self.execute_module, args=(self._clear_deck,), daemon=True
        ).start()

    def _clear_deck(self):
        """Lazy load and execute gig_close module"""
        import gig_close  # Heavy module imported only when needed

        gig_close.main(
            shutdown_after=bool(self.settings.get("shutdown_after_clear")),
            manage_defender=bool(self.settings.get("manage_defender", True)),
        )

    def execute_module(self, target_function):
        try:
            target_function()
        except Exception as e:
            print(f"\nSYSTEM ERROR: {e}")
        finally:
            console_queue.put("__UNLOCK__")

    # --- Settings Logic ---
    def update_settings(self, phone, hotspot, path):
        if not phone.strip() or not hotspot.strip():
            print("Warning: Phone Name and Hotspot Name are mandatory.")
            return

        # Updated in place rather than rebuilt: a fresh dict here would drop
        # midi_device and shutdown_after_clear on every save.
        #
        # The device falls back to the stored name because the dropdown reads
        # empty whenever the controller is unplugged. Taking it at face value
        # meant saving any unrelated setting with the controller detached threw
        # the remembered device away.
        self.settings.update(
            {
                "phone_name": phone.strip(),
                "hotspot_name": hotspot.strip(),
                "osc_file_path": path.strip(),
                "midi_device": self.app.midi_device
                or self.settings.get("midi_device", ""),
                "audio_device": self.app.audio_device
                or self.settings.get("audio_device", ""),
            }
        )

        save_settings(self.settings)
        # Follow what was just saved, so a newly chosen device takes effect
        # without waiting for the next settings open or a restart.
        self.refresh_midi_devices()
        self.refresh_audio_devices()
        print("\nSettings successfully updated and saved to settings.json.")

        # Close the modal by updating the Slint state property
        self.app.show_settings = False

    def update_shutdown_option(self, enabled):
        """Persist the shutdown toggle the moment it is flipped — it lives on the
        main screen, so there is no Save button to carry it."""
        self.settings["shutdown_after_clear"] = bool(enabled)
        save_settings(self.settings)
        print()
        print(
            "Shutdown after CLEAR DECK: "
            + ("ON — the PC will power off." if enabled else "off.")
        )

    def update_defender_option(self, enabled):
        """Persist the Defender button the moment it is pressed — it lives on the
        main screen, so there is no Save button to carry it."""
        self.settings["manage_defender"] = bool(enabled)
        save_settings(self.settings)
        print()
        print(
            "Windows Defender step: "
            + ("ON — LAUNCH and CLEAR DECK will toggle it."
               if enabled else "off — both will skip it.")
        )

    def run(self):
        self.app.run()


# ==========================================
# STARTUP SEQUENCE
# ==========================================
# How often the splash advances a step. Long enough that the animation gets a
# frame or two between the blocking pieces, short enough not to pad startup.
STEP_INTERVAL = datetime.timedelta(milliseconds=60)

# How long to wait for the backend before deciding a splash is warranted.
#
# Measured warm, listing devices takes ~170ms on the worker while building the
# UI takes longer than that on the UI thread, so the probe is usually finished
# before this is even reached and the splash never appears. A cold boot blows
# straight through it, which is the case the splash exists for. Nothing is on
# screen while this runs, so it costs latency and no visible stutter.
READY_GRACE = 0.1

# Logical size of the main window, mirroring preferred-width/height in
# ui/app.slint. Kept here because the window has to be sized explicitly once a
# splash exists — see window_setup._resize.
MAIN_WINDOW_SIZE = (800, 600)


def _warm_up(done):
    """Pre-import the launch module, off the UI thread.

    Measured at 958ms: cv2, pyautogui and numpy, pulled in by gig_launcher.
    That used to be paid on the user's first LAUNCH click; behind the splash it
    is paid where it can be watched instead. gig_launcher is only function
    definitions, so importing it early has no side effects.

    Runs on its own thread, so it must not touch Slint — it only sets an Event.
    """
    try:
        import gig_launcher  # noqa: F401
    except Exception as e:
        # A failed warm-up must never strand the user on the splash. The click
        # path imports it again and will report the real error there.
        print(f"Warning: could not pre-load the launch module: {e}")
    finally:
        done.set()


def _probe_devices(found, done):
    """List the devices, off the UI thread.

    Listing is the slow part of starting up and touches no Slint object, so it
    runs here while the UI is being built. Opening the chosen device stays on
    the UI thread, where the models and shared state live.

    CoInitialize first: audio_input goes through pycaw, and COM has to be live
    on whichever thread touches it — the same rule AudioMeter._watch follows.
    """
    try:
        comtypes.CoInitialize()
        try:
            found["midi"] = midi_input.list_devices()
            found["audio"] = audio_input.list_devices()
        finally:
            comtypes.CoUninitialize()
    except Exception as e:
        # Startup must not hang on a probe that died. Empty lists mean the app
        # opens with no devices, which the settings modal can put right.
        print(f"Warning: could not list devices: {e}")
    finally:
        done.set()


def run_with_splash():
    """Start the app, showing a splash only if the backend is slow.

    The backend is probed on a worker thread while the UI is built. If it has
    finished by the time the UI is ready, the main window opens straight away
    and no splash is ever created — the warm-restart case. Otherwise the splash
    holds the screen until the probe lands, which is what a cold boot needs.

    Either way the main window is not shown until its devices are set up, so
    there is never a window to click into while it is still starting.
    """
    found = {}
    probe_done = threading.Event()
    threading.Thread(
        target=_probe_devices, args=(found, probe_done), daemon=True
    ).start()

    # Started here rather than with the devices because it is not part of being
    # ready: the launch module only matters on the first LAUNCH click, and
    # waiting for it would put the splash up on every start.
    warm_done = threading.Event()
    threading.Thread(target=_warm_up, args=(warm_done,), daemon=True).start()

    app = GigControlApp()
    # Held: dropping either timer stops the retries that centre the windows.
    held = {}

    def hand_over():
        """Set the devices up, then put the main window on screen."""
        app.apply_midi_devices(found.get("midi", []), announce=True)
        app.apply_audio_devices(found.get("audio", []), announce=True)
        app.app.show()
        # The size is forced because a Slint window that is not the only one on
        # screen does not take its preferred-width/height — measured at 668x398
        # against 1018x797 on its own. It matches app.slint's preferred size.
        held["window_timer"] = window_setup.apply(size=MAIN_WINDOW_SIZE)

    # The whole question: did the backend finish while the UI was being built?
    # Nothing is on screen yet, so waiting here is latency, not a freeze.
    if probe_done.wait(READY_GRACE):
        hand_over()
        slint.run_event_loop()
        return

    splash = load_ui().SplashWindow()
    splash.logo = slint.Image.load_from_path(resource_path("assets/splash_logo.png"))
    splash.shine = slint.Image.load_from_path(resource_path("assets/splash_shine.png"))
    splash.status = "Preparing devices…"
    splash.show()
    held["splash_timer"] = window_setup.apply()

    timer = slint.Timer()

    def pump():
        if not probe_done.is_set():
            return
        timer.stop()
        # Shown before the splash is hidden, so the event loop is never left
        # without a window — that would end it and close the app.
        hand_over()
        splash.hide()

    timer.start(slint.TimerMode.Repeated, STEP_INTERVAL, pump)
    slint.run_event_loop()


# ==========================================
# APP ENTRY POINT
# ==========================================
if __name__ == "__main__":
    import ctypes

    # Keep the taskbar grouping ID if desired
    my_app_id = "gig_control.rig.launcher.1"
    ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(my_app_id)

    run_with_splash()

