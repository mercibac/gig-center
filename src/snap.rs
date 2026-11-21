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
    if let Some(hwnd) = find_window_to_snap(osc_name, OPEN_STAGE_CONTROL_WINDOW_DEFAULT) {
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
    if let Some(hwnd) = find_window_to_snap(gig_name, GIG_PERFORMER_WINDOW_DEFAULT) {
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

/// Every visible window whose title contains the substring, in the order
/// `EnumWindows` hands them over — which is front to back in the z-order.
///
/// Collects them all rather than stopping at the first, because more than one
/// window can match and the first is not always the one wanted; see
/// [`preferred_window`]. Walking the rest of the list costs nothing beside the
/// enumeration already under way.
fn matching_windows(title_substring: &str) -> Vec<(String, HWND)> {
    struct SearchData {
        target: String,
        found: Vec<(String, HWND)>,
    }

    let mut data = SearchData {
        target: title_substring.to_string(),
        found: Vec::new(),
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut SearchData);

        if IsWindowVisible(hwnd).as_bool() {
            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&title_buf[..len as usize]);
                if title_matches(&title, &data.target) {
                    data.found.push((title, hwnd));
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

/// Find a window whose title contains the given substring (case-insensitive),
/// the frontmost one where several match.
///
/// What the F5 step wants: at the moment it runs, the launcher is the only
/// Open Stage Control window there is, and it is the one that has to come
/// forward.
pub fn find_window_by_title(title_substring: &str) -> Option<HWND> {
    matching_windows(title_substring)
        .into_iter()
        .next()
        .map(|(_, hwnd)| hwnd)
}

/// Which of the matching windows to snap, as an index into `titles`.
///
/// Open Stage Control runs two windows at once — the launcher that starts the
/// server, and the surface carrying the controls. They share a process and a
/// window class, so nothing but the title separates them, and both contain
/// the name being searched for:
///
/// | Window | Title |
/// | --- | --- |
/// | launcher | `Open Stage Control` |
/// | surface  | `Open Stage Control v1.30.1 (PSC LIVE MERBAC-MP.json)` |
///
/// The launcher's whole title is the bare product name; the surface appends
/// the version and the loaded session. Since `EnumWindows` reports front to
/// back, which of the two got snapped came down to which had been clicked
/// last — and snapping the launcher would leave the controls wherever they
/// happened to be, mid-set, with nothing reported as having gone wrong.
///
/// Compared against `built_in` and not against what was searched for, which
/// is the whole reason this is safe to apply to both programs. The Gig
/// Performer field on this rig holds the exact title of its window, so
/// comparing with what was typed would push that window to the back the
/// moment a plugin editor opened carrying the gig's name in its own title.
/// `built_in` is a constant in this file; no setting can collide with it by
/// being precise.
///
/// Every match being the bare name leaves the frontmost one, which is what
/// this did before.
pub fn preferred_window(titles: &[&str], built_in: &str) -> Option<usize> {
    if titles.is_empty() {
        return None;
    }
    let built_in = built_in.trim().to_lowercase();
    let specific = titles
        .iter()
        .position(|title| title.trim().to_lowercase() != built_in);
    Some(specific.unwrap_or(0))
}

/// Find the window to snap, preferring the surface window over Open Stage
/// Control's launcher. See [`preferred_window`] for the rule.
pub fn find_window_to_snap(title_substring: &str, built_in: &str) -> Option<HWND> {
    let matches = matching_windows(title_substring);
    let titles: Vec<&str> = matches.iter().map(|(title, _)| title.as_str()).collect();
    preferred_window(&titles, built_in).map(|at| matches[at].1)
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

    /// The two windows as they actually read on this rig, taken off a running
    /// Open Stage Control 1.30.1. Both contain the built-in name, both belong
    /// to one process under one window class, so the title is the only thing
    /// that separates them.
    const OSC_LAUNCHER: &str = "Open Stage Control";
    const OSC_SURFACE: &str = "Open Stage Control v1.30.1 (PSC LIVE MERBAC-MP.json)";

    #[test]
    fn nothing_matching_is_nothing_to_snap() {
        assert_eq!(
            preferred_window(&[], OPEN_STAGE_CONTROL_WINDOW_DEFAULT),
            None
        );
    }

    #[test]
    fn a_single_match_is_the_one_used() {
        assert_eq!(
            preferred_window(&[OSC_SURFACE], OPEN_STAGE_CONTROL_WINDOW_DEFAULT),
            Some(0)
        );
    }

    /// The bug. The launcher was clicked during the thirty seconds before the
    /// snap, so it is in front, and it used to be the window that got dragged
    /// into Open Stage Control's share of the screen — leaving the controls
    /// wherever they were.
    #[test]
    fn the_launcher_in_front_does_not_win_over_the_surface() {
        assert_eq!(
            preferred_window(
                &[OSC_LAUNCHER, OSC_SURFACE],
                OPEN_STAGE_CONTROL_WINDOW_DEFAULT
            ),
            Some(1)
        );
    }

    /// The ordinary case, which has to keep working: the surface opens after
    /// the launcher, so it is normally the one in front already.
    #[test]
    fn the_surface_in_front_is_still_the_one_snapped() {
        assert_eq!(
            preferred_window(
                &[OSC_SURFACE, OSC_LAUNCHER],
                OPEN_STAGE_CONTROL_WINDOW_DEFAULT
            ),
            Some(0)
        );
    }

    /// Only the launcher is up — the server never started, or the surface was
    /// closed. There is nothing better to pick, so it is picked, exactly as
    /// before this rule existed.
    #[test]
    fn the_launcher_alone_is_still_snapped() {
        assert_eq!(
            preferred_window(&[OSC_LAUNCHER], OPEN_STAGE_CONTROL_WINDOW_DEFAULT),
            Some(0)
        );
    }

    /// Why the comparison is against the built-in name and not against what
    /// was searched for. The Gig Performer field on this rig holds
    /// "Keyboard_Live_USB", the exact title of the window it names; against
    /// the search term that window would count as the generic one and lose to
    /// a plugin editor carrying the gig's name.
    #[test]
    fn an_exactly_named_gig_performer_window_still_wins() {
        let titles = ["Keyboard_Live_USB", "Keyboard_Live_USB - Kontakt 7"];
        assert_eq!(
            preferred_window(&titles, GIG_PERFORMER_WINDOW_DEFAULT),
            Some(0),
            "the window the setting names lost to a plugin editor"
        );
    }

    /// A title is matched case-insensitively and trimmed everywhere else in
    /// this file; the bare name is recognised the same way.
    #[test]
    fn the_bare_name_is_recognised_whatever_its_case_or_padding() {
        assert_eq!(
            preferred_window(
                &["  OPEN STAGE CONTROL  ", OSC_SURFACE],
                OPEN_STAGE_CONTROL_WINDOW_DEFAULT
            ),
            Some(1)
        );
    }
}
