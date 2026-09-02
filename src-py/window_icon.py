"""Title bar / taskbar icon for the main window.

Winit creates its windows without an icon, so Windows draws the generic
placeholder in the title bar. The icon bundled into the exe is what Explorer and
the shell read, not the window itself.

The Rust build fixes this inside winit, before the window exists (see
src-rs/app_icon.rs). Slint's Python binding exposes no equivalent, so the icon
is handed to the window here instead, through WM_SETICON — which needs the
window to already be there.
"""

import ctypes
import datetime
import os
from ctypes import wintypes

import slint

from utils import resource_path

WM_SETICON = 0x0080
ICON_SMALL = 0
ICON_BIG = 1
IMAGE_ICON = 1
LR_LOADFROMFILE = 0x0010

# Title bar size, then taskbar / Alt+Tab size.
SM_CXSMICON, SM_CYSMICON = 49, 50
SM_CXICON, SM_CYICON = 11, 12

# `show()` only asks for a window; winit creates the real one from inside the
# event loop, a few cycles later. Poll for it, and give up rather than spin
# forever — a missing icon is a blemish, not a reason to fail the launch.
RETRY_INTERVAL = datetime.timedelta(milliseconds=100)
MAX_ATTEMPTS = 20

_user32 = ctypes.windll.user32

# Without these the 64-bit handles come back through the default c_int return
# type and are silently truncated.
_user32.LoadImageW.restype = wintypes.HANDLE
_user32.LoadImageW.argtypes = [
    wintypes.HINSTANCE,
    wintypes.LPCWSTR,
    wintypes.UINT,
    ctypes.c_int,
    ctypes.c_int,
    wintypes.UINT,
]
_user32.SendMessageW.restype = wintypes.LPARAM
_user32.SendMessageW.argtypes = [
    wintypes.HWND,
    wintypes.UINT,
    wintypes.WPARAM,
    wintypes.LPARAM,
]
_user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, wintypes.LPDWORD]

_WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


def _main_window():
    """The first visible, titled top-level window belonging to this process."""
    found = []

    def visit(hwnd, _lparam):
        pid = wintypes.DWORD()
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if (
            pid.value == os.getpid()
            and _user32.IsWindowVisible(hwnd)
            and _user32.GetWindowTextLengthW(hwnd) > 0
        ):
            found.append(hwnd)
            return False  # stop the enumeration
        return True

    _user32.EnumWindows(_WNDENUMPROC(visit), 0)
    return found[0] if found else None


def _set_icon(hwnd, ico_path):
    """Attach the icon at both sizes Windows asks for. False if it would not load."""
    sizes = (
        (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
        (ICON_BIG, SM_CXICON, SM_CYICON),
    )
    for which, cx_metric, cy_metric in sizes:
        handle = _user32.LoadImageW(
            None,
            ico_path,
            IMAGE_ICON,
            _user32.GetSystemMetrics(cx_metric),
            _user32.GetSystemMetrics(cy_metric),
            LR_LOADFROMFILE,
        )
        if not handle:
            return False
        _user32.SendMessageW(hwnd, WM_SETICON, which, handle)
    return True


def apply():
    """Start giving the main window the app icon, and return the timer driving it.

    The caller has to hold on to the returned timer: dropping it stops the
    retries before the window has been found. Returns None when there is no
    icon to install.
    """
    ico_path = resource_path("assets/icon.ico")
    if not os.path.exists(ico_path):
        return None

    timer = slint.Timer()
    attempts = 0

    def tick():
        nonlocal attempts
        attempts += 1
        hwnd = _main_window()
        if (hwnd and _set_icon(hwnd, ico_path)) or attempts >= MAX_ATTEMPTS:
            timer.stop()

    timer.start(slint.TimerMode.Repeated, RETRY_INTERVAL, tick)
    return timer
