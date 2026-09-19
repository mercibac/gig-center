"""Window dressing Slint's Python binding cannot do: the title bar icon, and
centring the window on screen.

Both need the real window, and there is no Python hook for its creation: `show()`
only asks for one, and winit creates it from inside the event loop a few cycles
later. So both are applied together, as soon as the window can be found.

The Rust build does the equivalent work through winit itself, before the window
is ever composited — see src-rs/app_icon.rs and src-rs/center.rs.
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

SWP_NOSIZE = 0x0001
SWP_NOZORDER = 0x0004
SWP_NOACTIVATE = 0x0010

# Primary monitor, then title bar icon size, then taskbar / Alt+Tab size.
SM_CXSCREEN, SM_CYSCREEN = 0, 1
SM_CXSMICON, SM_CYSMICON = 49, 50
SM_CXICON, SM_CYICON = 11, 12

# Polled rather than waited on, because nothing signals the window's arrival.
# Kept to one frame at 60Hz so the window is placed as close to its first
# appearance as this can get, and capped so a window that never shows up does
# not leave a timer running for the life of the app.
RETRY_INTERVAL = datetime.timedelta(milliseconds=16)
MAX_ATTEMPTS = 120

# Our own handle on user32, deliberately not the cached ctypes.windll one.
# ctypes.windll is process-wide, so setting argtypes on it rewrites those
# signatures for every library in the process. Doing that to GetWindowRect broke
# pygetwindow — it passes a pointer to its own RECT class — which aborted the
# EnumWindows walk in CLEAR DECK and left it closing nothing at all.
_user32 = ctypes.WinDLL("user32")

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
_user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
_user32.SetWindowPos.argtypes = [
    wintypes.HWND,
    wintypes.HWND,
    ctypes.c_int,
    ctypes.c_int,
    ctypes.c_int,
    ctypes.c_int,
    wintypes.UINT,
]

_WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


def _main_window():
    """This process's top-level window, or None before it exists.

    Matched on having a title: winit also owns an untitled helper window and an
    IME window, and both are there from the start. Visibility is deliberately
    not required — catching the window the moment it is created, rather than
    once it is on screen, is what keeps the centring from being seen.
    """
    found = []

    def visit(hwnd, _lparam):
        pid = wintypes.DWORD()
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value == os.getpid() and _user32.GetWindowTextLengthW(hwnd) > 0:
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


def _center(hwnd):
    """Put the window in the middle of the primary monitor.

    Measured with GetWindowRect, which reports the outer frame, so the border
    and title bar count towards the centring rather than pushing the window
    down and right.
    """
    frame = wintypes.RECT()
    if not _user32.GetWindowRect(hwnd, ctypes.byref(frame)):
        return False

    width = frame.right - frame.left
    height = frame.bottom - frame.top
    screen_width = _user32.GetSystemMetrics(SM_CXSCREEN)
    screen_height = _user32.GetSystemMetrics(SM_CYSCREEN)

    return bool(
        _user32.SetWindowPos(
            hwnd,
            None,
            (screen_width - width) // 2,
            (screen_height - height) // 2,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    )


def apply():
    """Start dressing the main window, and return the timer driving the retries.

    The caller has to hold on to the returned timer: dropping it stops the
    retries before the window has been found.
    """
    ico_path = resource_path("assets/icon.ico")
    timer = slint.Timer()
    attempts = 0

    def tick():
        nonlocal attempts
        attempts += 1
        hwnd = _main_window()
        if hwnd is not None:
            _center(hwnd)
            if os.path.exists(ico_path):
                _set_icon(hwnd, ico_path)
            timer.stop()
        elif attempts >= MAX_ATTEMPTS:
            timer.stop()

    timer.start(slint.TimerMode.Repeated, RETRY_INTERVAL, tick)
    return timer
