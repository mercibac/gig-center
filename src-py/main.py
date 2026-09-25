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
        self.refresh_midi_devices(announce=True)

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
        devices = midi_input.list_devices()
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
        devices = audio_input.list_devices()
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


def run_with_splash():
    """Show the splash, do the slow work behind it, then hand over.

    The main window is never shown until everything is ready, so there is no
    window to click into while it is still starting.

    Each step announces itself and then yields to the event loop before doing
    its work, so the label is on screen while the work happens rather than
    after it.
    """
    splash = load_ui().SplashWindow()
    splash.logo = slint.Image.load_from_path(resource_path("assets/splash_logo.png"))
    splash.shine = slint.Image.load_from_path(resource_path("assets/splash_shine.png"))
    splash.show()

    # Held: dropping either timer stops the retries that centre the windows.
    held = {"splash_timer": window_setup.apply()}

    warm_done = threading.Event()
    threading.Thread(target=_warm_up, args=(warm_done,), daemon=True).start()

    def build_app():
        held["app"] = GigControlApp()

    def list_audio():
        held["app"].refresh_audio_devices(announce=True)

    steps = [
        ("Starting MIDI…", build_app),
        ("Listing audio outputs…", list_audio),
    ]

    timer = slint.Timer()
    state = {"step": 0, "announced": False}

    def pump():
        step = state["step"]

        if step >= len(steps):
            # Everything on the UI thread is done; the import may still be
            # running, and the splash waits for it rather than handing over to
            # a window whose first click would stall.
            if not warm_done.is_set():
                splash.status = "Loading launch module…"
                return
            timer.stop()
            hand_over()
            return

        label, work = steps[step]
        if not state["announced"]:
            splash.status = label
            state["announced"] = True
            return  # let it paint before blocking on the work

        work()
        state["step"] += 1
        state["announced"] = False

    def hand_over():
        app = held["app"]
        # Shown before the splash is hidden, so the event loop is never left
        # without a window — that would end it and close the app.
        app.app.show()
        # The size is forced because a Slint window that is not the only one on
        # screen does not take its preferred-width/height — measured at 668x398
        # against 1018x797 on its own. It matches app.slint's preferred size.
        held["window_timer"] = window_setup.apply(size=MAIN_WINDOW_SIZE)
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

