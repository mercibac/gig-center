import slint
import threading
import queue
import os
import sys
import json
import datetime

# Lightweight config import
from utils import CONFIG_FILE, bundled_settings, ensure_settings_file, resource_path
import midi_input
import window_setup


# --- Configuration Manager ---
DEFAULT_CONFIG = {
    "phone_name": "SMART PHONE",
    "hotspot_name": "SHARED ACCESS POINT",
    "osc_file_path": "",
    "midi_device": "",
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

        # Load Slint UI (optimized path)
        components = slint.load_file(resource_path("ui/app.slint"))
        self.app = components.MainWindow()

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

        # Held because dropping the timer would stop the retries before the
        # window winit creates has appeared.
        self.window_timer = window_setup.apply()

        # Connect UI Callbacks to Python functions
        self.app.launch_clicked = self.run_launch_script
        self.app.clear_clicked = self.run_clear_script
        self.app.save_clicked = self.update_settings
        self.app.shutdown_toggled = self.update_shutdown_option
        self.app.defender_toggled = self.update_defender_option
        self.app.settings_opened = self.refresh_midi_devices

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
        self.app.midi_active = self.midi.is_active()

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
        self.settings.update(
            {
                "phone_name": phone.strip(),
                "hotspot_name": hotspot.strip(),
                "osc_file_path": path.strip(),
                "midi_device": self.app.midi_device,
            }
        )

        save_settings(self.settings)
        self.refresh_midi_devices()
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
# APP ENTRY POINT
# ==========================================
if __name__ == "__main__":
    import ctypes

    # Keep the taskbar grouping ID if desired
    my_app_id = "gig_control.rig.launcher.1"
    ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(my_app_id)

    app = GigControlApp()
    app.run()

