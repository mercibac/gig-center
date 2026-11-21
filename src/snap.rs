use crate::settings::WindowNames;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetSystemMetrics, GetWindowTextW, IsWindowVisible, MoveWindow, ShowWindow,
    SM_CXSCREEN, SM_CYSCREEN, SW_RESTORE,
};

/// What Open Stage Control calls its own window. It does not change.
pub const OPEN_STAGE_CONTROL_WINDOW_DEFAULT: &str = "Open Stage Control";

/// What Gig Performer's title bar reads on this rig. It carries the name of the
/// loaded gig file, so this one goes stale the moment a gig file is renamed —
/// which is the reason the Advanced section can override it.
pub const GIG_PERFORMER_WINDOW_DEFAULT: &str = "Keyboard_Live";

/// The window title to search for: the one set in the Advanced settings, or the
/// default above when that field is empty.
///
/// The empty case is not a nicety. [`find_window_by_title`] matches with
/// `contains`, and every string contains `""`, so an empty field would match
/// the first visible window that has a title at all — most likely Gig Center
/// itself, which would then be the thing dragged into half the screen.
///
/// Unlike the executable paths there is nothing to check here: whether a window
/// by that name exists is only knowable at the moment of the search, and the
/// callers below already say so when it is not found.
pub fn window_name<'a>(chosen: &'a str, default: &'a str) -> &'a str {
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return default;
    }
    chosen
}

/// Snap gig windows into left/right split positions.
/// Open Stage Control → left half, Gig Performer → right half.
pub fn snap_gig_windows(names: &WindowNames) {
    console_log!("Snapping windows into position...");

    let osc_name = window_name(&names.open_stage_control, OPEN_STAGE_CONTROL_WINDOW_DEFAULT);
    let gig_name = window_name(&names.gig_performer, GIG_PERFORMER_WINDOW_DEFAULT);

    // Give apps time to register their windows with the OS
    thread::sleep(Duration::from_secs(2));

    let (screen_width, screen_height) =
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let half_width = screen_width / 2;

    // Snap Open Stage Control to the left half
    if let Some(hwnd) = find_window_by_title(osc_name) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = MoveWindow(hwnd, 0, 0, half_width, screen_height, BOOL(1));
        }
        console_log!("Open Stage Control anchored to the left.");
    } else {
        // The name searched for goes in the message because it is a setting
        // now: an app that never started and a name that was mistyped look
        // exactly alike from here otherwise.
        console_log!(
            "Could not find an Open Stage Control window matching \"{osc_name}\" to snap."
        );
    }

    // Snap Gig Performer to the right half
    if let Some(hwnd) = find_window_by_title(gig_name) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = MoveWindow(hwnd, half_width, 0, half_width, screen_height, BOOL(1));
        }
        console_log!("Gig Performer anchored to the right.");
    } else {
        console_log!("Could not find a Gig Performer window matching \"{gig_name}\" to snap.");
    }
}

/// Whether a window's title counts as a match for what was searched for.
///
/// Pulled out of the enumeration callback below so the rule can be tested
/// without Win32. It lowercases the needle once per window rather than once per
/// search, which is nothing beside the cost of walking every top-level window.
fn title_matches(title: &str, target: &str) -> bool {
    title.to_lowercase().contains(&target.to_lowercase())
}

/// Find a window whose title contains the given substring (case-insensitive).
/// Uses Win32 EnumWindows to iterate all top-level windows.
pub fn find_window_by_title(title_substring: &str) -> Option<HWND> {
    struct SearchData {
        target: String,
        found: Option<HWND>,
    }

    let mut data = SearchData {
        target: title_substring.to_string(),
        found: None,
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut SearchData);

        if IsWindowVisible(hwnd).as_bool() {
            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&title_buf[..len as usize]);
                if title_matches(&title, &data.target) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_field_uses_the_built_in_name() {
        assert_eq!(
            window_name("", GIG_PERFORMER_WINDOW_DEFAULT),
            GIG_PERFORMER_WINDOW_DEFAULT
        );
    }

    /// Whitespace is not a choice. It matters more here than for the executable
    /// paths: " Keyboard_Live" is a substring search that can never match,
    /// because Gig Performer's title has no space in front of the gig name.
    #[test]
    fn a_blank_field_uses_the_built_in_name() {
        assert_eq!(
            window_name("   ", OPEN_STAGE_CONTROL_WINDOW_DEFAULT),
            OPEN_STAGE_CONTROL_WINDOW_DEFAULT
        );
    }

    #[test]
    fn a_chosen_name_is_used() {
        assert_eq!(
            window_name("Wedding_Set", GIG_PERFORMER_WINDOW_DEFAULT),
            "Wedding_Set"
        );
    }

    #[test]
    fn a_chosen_name_is_trimmed() {
        assert_eq!(
            window_name("  Wedding_Set  ", GIG_PERFORMER_WINDOW_DEFAULT),
            "Wedding_Set"
        );
    }

    #[test]
    fn a_title_matches_whatever_its_case() {
        assert!(title_matches("Open Stage Control", "open stage control"));
        assert!(title_matches("open stage control", "Open Stage Control"));
    }

    /// The whole point of a substring match: Gig Performer's title is the gig
    /// file's name plus whatever else it decides to put there.
    #[test]
    fn part_of_a_title_is_enough() {
        assert!(title_matches(
            "Gig Performer 5 - Keyboard_Live.gig",
            "Keyboard_Live"
        ));
    }

    #[test]
    fn an_unrelated_title_does_not_match() {
        assert!(!title_matches("Gig Control Center", "Keyboard_Live"));
    }

    /// Why [`window_name`] exists. An empty field reaching this would snap
    /// whichever window happened to be enumerated first.
    #[test]
    fn an_empty_target_matches_every_title() {
        assert!(title_matches("Gig Control Center", ""));
        assert!(title_matches("anything at all", ""));
    }
}
