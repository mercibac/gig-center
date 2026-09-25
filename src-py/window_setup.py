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
SWP_NOMOVE = 0x0002
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
_user32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]

# Windows 10 1607 and later. Absent on older builds, where the system DPI is
# the best available answer.
try:
    _user32.GetDpiForWindow.argtypes = [wintypes.HWND]
    _user32.GetDpiForWindow.restype = wintypes.UINT
    _HAS_WINDOW_DPI = True
except AttributeError:  # pragma: no cover - depends on the Windows build
    _HAS_WINDOW_DPI = False
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


# Windows already dressed, so a second apply() moves on to the next one rather
# than finding the first again. The splash and the main window share a title,
# and hiding the splash does not remove it from EnumWindows, so without this
# the main window would never be centred.
_dressed = set()


def _has_area(hwnd):
    """Whether the window occupies any space at all.

    A title is not enough to identify the app's own window. Enumerated live,
    this process owns four titled windows: the two real ones, plus 'MSCTFIME
    UI' and 'Default IME', which Windows creates for input methods. Both of
    those measure 0x0, and neither should ever be moved or given an icon.
    """
    frame = wintypes.RECT()
    if not _user32.GetWindowRect(hwnd, ctypes.byref(frame)):
        return False
    return frame.right > frame.left and frame.bottom > frame.top


def _next_window():
    """This process's next undressed top-level window, or None.

    Visibility is deliberately not required — catching the window the moment it
    is created, rather than once it is on screen, is what keeps the centring
    from being seen.
    """
    found = []

    def visit(hwnd, _lparam):
        pid = wintypes.DWORD()
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if (
            pid.value == os.getpid()
            and _user32.GetWindowTextLengthW(hwnd) > 0
            and hwnd not in _dressed
            and _has_area(hwnd)
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


def _physical_size(hwnd, logical):
    """Physical window size for a logical client size, at this window's DPI.

    The frame — border and title bar — is measured rather than assumed: it is
    whatever the window rect has that the client rect does not.
    """
    client = wintypes.RECT()
    frame = wintypes.RECT()
    if not _user32.GetClientRect(hwnd, ctypes.byref(client)):
        return None
    if not _user32.GetWindowRect(hwnd, ctypes.byref(frame)):
        return None

    dpi = _user32.GetDpiForWindow(hwnd) if _HAS_WINDOW_DPI else 0
    scale = (dpi or 96) / 96
    border_w = (frame.right - frame.left) - (client.right - client.left)
    border_h = (frame.bottom - frame.top) - (client.bottom - client.top)
    return (
        round(logical[0] * scale) + border_w,
        round(logical[1] * scale) + border_h,
    )


def _resize(hwnd, logical):
    """Give the window its intended size.

    Needed because a Slint window that is not the only one on screen does not
    take its preferred-width/height: measured, the main window opened at
    668x398 when a splash existed, against 1018x797 on its own. The Python
    binding exposes no way to resize — setting .width and .height on the
    component does nothing — so it is done here, with the rest of the dressing
    Slint cannot do.
    """
    size = _physical_size(hwnd, logical)
    if size is None:
        return False
    return bool(
        _user32.SetWindowPos(
            hwnd, None, 0, 0, size[0], size[1],
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    )


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


def apply(size=None):
    """Start dressing the next undressed window, and return the retry timer.

    Called once for the splash and again for the main window; each call takes
    the next window this process has not already handled.

    `size` is a logical (width, height) to force, for a window that will not
    take its own preferred size — see _resize. Omit it to leave the size alone.

    The caller has to hold on to the returned timer: dropping it stops the
    retries before the window has been found.
    """
    ico_path = resource_path("assets/icon.ico")
    timer = slint.Timer()
    attempts = 0

    def tick():
        nonlocal attempts
        attempts += 1
        hwnd = _next_window()
        if hwnd is not None:
            _dressed.add(hwnd)
            if size is not None:
                _resize(hwnd, size)
            # Centred after any resize, so the new size is what gets centred.
            _center(hwnd)
            if os.path.exists(ico_path):
                _set_icon(hwnd, ico_path)
            timer.stop()
        elif attempts >= MAX_ATTEMPTS:
            timer.stop()

    timer.start(slint.TimerMode.Repeated, RETRY_INTERVAL, tick)
    return timer
