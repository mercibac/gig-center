import slint
import threading
import queue
import os
import sys
import json
import datetime

# Lightweight config import
from utils import CONFIG_FILE, bundled_settings, ensure_settings_file, resource_path
import window_icon


# --- Configuration Manager ---
DEFAULT_CONFIG = {
    "phone_name": "SMART PHONE",
    "hotspot_name": "SHARED ACCESS POINT",
    "osc_file_path": "",
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

        self.app.console_text = "System initialized. Awaiting command...\n"

        sys.stdout = ConsoleRedirector()
        sys.stderr = ConsoleRedirector()

        # Optimized timer: 100ms instead of 50ms reduces overhead
        self.console_timer = slint.Timer()
        self.console_timer.start(
            slint.TimerMode.Repeated,
            datetime.timedelta(milliseconds=100),  # Reduced from 50ms to 100ms
            self.poll_console,
        )

        # Held because dropping the timer would stop the retries before the
        # window winit creates has appeared.
        self.icon_timer = window_icon.apply()

        # Connect UI Callbacks to Python functions
        self.app.launch_clicked = self.run_launch_script
        self.app.clear_clicked = self.run_clear_script
        self.app.save_clicked = self.update_settings

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
        self.lock_buttons("INITIALIZING...")
        self.app.console_text = ""
        # Lazy import of heavy module
        threading.Thread(
            target=self.execute_module, args=(self._launch_gig,), daemon=True
        ).start()

    def _launch_gig(self):
        """Lazy load and execute gig_launcher module"""
        import gig_launcher  # Heavy module imported only when needed

        gig_launcher.main()

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

        gig_close.main()

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

        self.settings = {
            "phone_name": phone.strip(),
            "hotspot_name": hotspot.strip(),
            "osc_file_path": path.strip(),
        }

        save_settings(self.settings)
        print("\nSettings successfully updated and saved to settings.json.")

        # Close the modal by updating the Slint state property
        self.app.show_settings = False

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

