use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetSystemMetrics, GetWindowTextW, IsWindowVisible, MoveWindow, ShowWindow,
    SM_CXSCREEN, SM_CYSCREEN, SW_RESTORE,
};

/// Snap gig windows into left/right split positions.
/// Open Stage Control → left half, Gig Performer → right half.
pub fn snap_gig_windows() {
    thread::sleep(Duration::from_secs(30));
    console_log!("Snapping windows into position...");

    // Give apps time to register their windows with the OS
    thread::sleep(Duration::from_secs(2));

    let (screen_width, screen_height) = unsafe {
        (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    };
    let half_width = screen_width / 2;

    // Snap Open Stage Control to the left half
    if let Some(hwnd) = find_window_by_title("Open Stage Control") {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = MoveWindow(hwnd, 0, 0, half_width, screen_height, BOOL(1));
        }
        console_log!("Open Stage Control anchored to the left.");
    } else {
        console_log!("Could not find Open Stage Control window to snap.");
    }

    // Snap Gig Performer to the right half
    if let Some(hwnd) = find_window_by_title("Keyboard_Live") {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = MoveWindow(hwnd, half_width, 0, half_width, screen_height, BOOL(1));
        }
        console_log!("Gig Performer anchored to the right.");
    } else {
        console_log!("Could not find Gig Performer window to snap.");
    }
}

/// Find a window whose title contains the given substring (case-insensitive).
/// Uses Win32 EnumWindows to iterate all top-level windows.
pub fn find_window_by_title(title_substring: &str) -> Option<HWND> {
    struct SearchData {
        target: String,
        found: Option<HWND>,
    }

    let mut data = SearchData {
        target: title_substring.to_lowercase(),
        found: None,
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut SearchData);

        if IsWindowVisible(hwnd).as_bool() {
            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&title_buf[..len as usize]);
                if title.to_lowercase().contains(&data.target) {
                    data.found = Some(hwnd);
                    return BOOL(0); // Stop enumeration
                }
            }
        }
        BOOL(1) // Continue enumeration
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut data as *mut SearchData as isize),
        );
    }

    data.found
}
