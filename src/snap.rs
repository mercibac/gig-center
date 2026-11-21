use crate::settings::WindowNames;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetSystemMetrics, GetWindowRect, GetWindowTextW, IsWindowVisible,
    SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOP, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOACTIVATE, SW_RESTORE,
};

/// What Open Stage Control calls its own window. It does not change.
pub const OPEN_STAGE_CONTROL_WINDOW_DEFAULT: &str = "Open Stage Control";

/// What Gig Performer's title bar reads on this rig. It carries the name of the
/// loaded gig file, so this one goes stale the moment a gig file is renamed —
/// which is the reason the Advanced section can override it.
pub const GIG_PERFORMER_WINDOW_DEFAULT: &str = "Keyboard_Live";

/// Open Stage Control's share of the screen width, as a percentage. Gig
/// Performer gets whatever is left, which is why only one number is stored:
/// two fields could be set to a pair that does not cover the screen.
///
/// 29 rather than a round 50 because that is the split this rig plays — the
/// control surface wants roughly a third and Gig Performer wants the rest.
pub const SNAP_RATIO_DEFAULT_PERCENT: f32 = 29.0;

/// Narrower than this and a window is a sliver nobody can hit mid-set. The
/// floor earns its keep on typos more than on deliberate choices: `2` for `29`
/// would otherwise hide Open Stage Control rather than resize it, and that is
/// only visible once both programs are already up.
pub const SNAP_RATIO_MIN_PERCENT: f32 = 10.0;

/// The same floor seen from the other end, where Gig Performer is the sliver.
pub const SNAP_RATIO_MAX_PERCENT: f32 = 90.0;

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

/// What the Advanced ratio field resolved to.
///
/// A bare `f32` would carry the arithmetic perfectly well, but not the console
/// line: an empty field, a number pulled back into range and a typo all end up
/// holding a usable percentage, and once that has happened there is no way to
/// tell them apart. Saying which one it was is the same courtesy the "could not
/// find a window" lines below pay by naming the string they searched for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapWidth {
    /// The field was empty: the built-in share.
    BuiltIn,
    /// A number that was usable exactly as typed.
    Chosen(f32),
    /// A number outside the allowed range, pulled back to the nearest end.
    Clamped { asked: f32, used: f32 },
    /// Not a number at all. The built-in share is used.
    Unreadable,
}

impl SnapWidth {
    /// The percentage to actually snap with.
    pub fn percent(self) -> f32 {
        match self {
            SnapWidth::BuiltIn | SnapWidth::Unreadable => SNAP_RATIO_DEFAULT_PERCENT,
            SnapWidth::Chosen(percent) => percent,
            SnapWidth::Clamped { used, .. } => used,
        }
    }
}

/// Read the Advanced ratio field as Open Stage Control's share of the width.
///
/// Lenient about how the number is written, because the field is a plain text
/// box and every spelling it accepts is one the field itself invites: the
/// placeholder carries a `%`, and this rig's Windows is French, where the
/// numeric keypad types a comma where the decimal point goes.
///
/// Out of range is a separate answer from unreadable rather than one shared
/// fallback, because the two want different console lines: one names the
/// number used instead, the other says the field could not be read at all.
pub fn snap_width(chosen: &str) -> SnapWidth {
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return SnapWidth::BuiltIn;
    }

    let number = chosen
        .strip_suffix('%')
        .unwrap_or(chosen)
        .trim()
        .replace(',', ".");

    let Ok(asked) = number.parse::<f32>() else {
        return SnapWidth::Unreadable;
    };
    // Not redundant after a successful parse: Rust reads "inf" and "NaN" as
    // floats, and both would come back out of `clamp` looking like a width.
    if !asked.is_finite() {
        return SnapWidth::Unreadable;
    }

    let used = asked.clamp(SNAP_RATIO_MIN_PERCENT, SNAP_RATIO_MAX_PERCENT);
    if used == asked {
        SnapWidth::Chosen(asked)
    } else {
        SnapWidth::Clamped { asked, used }
    }
}

/// The two window widths for a screen this wide.
///
/// Gig Performer takes the remainder rather than a second multiplication, so
/// the pair always covers the screen exactly. Halving twice, which is what this
/// replaced, left a one-pixel strip of desktop down the middle of every
/// odd-width screen.
pub fn split_widths(screen_width: i32, percent: f32) -> (i32, i32) {
    let left = (screen_width as f32 * percent / 100.0).round() as i32;
    (left, screen_width - left)
}

/// How far a window's painted edges sit inside the rectangle Windows moves.
///
/// Since Vista a resizable window's rect carries an invisible resize border
/// that the drop shadow is drawn into: `GetWindowRect` counts it, the screen
/// does not. Two windows placed edge to edge by that rect therefore end up
/// with two borders' worth of nothing between them — the strip of desktop,
/// or of whatever was behind, that showed between Open Stage Control and Gig
/// Performer, measured at about 20px on this rig.
///
/// Windows' own snap compensates for exactly this, which is why dragging the
/// two windows into place by hand left no such gap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInsets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Measure one window's invisible border.
///
/// All zeroes when either call fails, which is the uncompensated placement
/// this replaced rather than a wrong one — a window nudged by a few pixels
/// beats a window not moved at all.
fn frame_insets(hwnd: HWND) -> FrameInsets {
    let mut outer = RECT::default();
    let mut painted = RECT::default();

    unsafe {
        if GetWindowRect(hwnd, &mut outer).is_err() {
            return FrameInsets::default();
        }
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            std::ptr::addr_of_mut!(painted).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .is_err()
        {
            return FrameInsets::default();
        }
    }

    // Clamped at zero: a painted edge outside the window rect would mean the
    // numbers disagree, and growing the window by a negative border would put
    // the gap back rather than close it.
    FrameInsets {
        left: (painted.left - outer.left).max(0),
        top: (painted.top - outer.top).max(0),
        right: (outer.right - painted.right).max(0),
        bottom: (outer.bottom - painted.bottom).max(0),
    }
}

/// Grow a rectangle of wanted *painted* pixels into the rectangle to hand to
/// Windows, so the window's visible edges land exactly where asked.
///
/// Split out from the move itself because this is the whole of the fix and
/// the only part of it that can be checked without two running programs.
pub fn outer_rect(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    insets: FrameInsets,
) -> (i32, i32, i32, i32) {
    (
        x - insets.left,
        y - insets.top,
        width + insets.left + insets.right,
        height + insets.top + insets.bottom,
    )
}

/// Move a window so its painted edges cover the given strip of the screen,
/// and lift it above whatever was in front of it.
///
/// `SetWindowPos` with `HWND_TOP` rather than `MoveWindow`: Open Stage Control
/// keeps a second, smaller launcher window that is never snapped, and it sat
/// in the z-order between these two — so every snap finished with Gig
/// Performer behind it. Raising both as they are placed buries it.
///
/// `SWP_NOACTIVATE` so the raising does not hand focus to whichever window
/// moved last; the caller decides who ends up in front.
fn place_window(hwnd: HWND, x: i32, width: i32, height: i32) {
    unsafe {
        // First, and before the insets are read: a maximized window ignores a
        // move, and the border it has while maximized is not the one it will
        // have once restored.
        let _ = ShowWindow(hwnd, SW_RESTORE);

        let (x, y, width, height) = outer_rect(x, 0, width, height, frame_insets(hwnd));
        let _ = SetWindowPos(hwnd, HWND_TOP, x, y, width, height, SWP_NOACTIVATE);
    }
}

/// Snap gig windows into their split positions.
/// Open Stage Control → the left share, Gig Performer → everything else.
pub fn snap_gig_windows(names: &WindowNames, snap_ratio: &str) {
    console_log!("Snapping windows into position...");

    let osc_name = window_name(&names.open_stage_control, OPEN_STAGE_CONTROL_WINDOW_DEFAULT);
    let gig_name = window_name(&names.gig_performer, GIG_PERFORMER_WINDOW_DEFAULT);

    // Said before anything moves, so the number reaches the console even if
    // neither window is ever found.
    let width = snap_width(snap_ratio);
    match width {
        SnapWidth::BuiltIn => console_log!(
            "Open Stage Control takes the built-in {SNAP_RATIO_DEFAULT_PERCENT}% of the width."
        ),
        SnapWidth::Chosen(percent) => {
            console_log!("Open Stage Control takes {percent}% of the width.")
        }
        SnapWidth::Clamped { asked, used } => console_log!(
            "A width of {asked}% is outside {SNAP_RATIO_MIN_PERCENT}-{SNAP_RATIO_MAX_PERCENT}%; using {used}% instead."
        ),
        SnapWidth::Unreadable => console_log!(
            "Could not read \"{snap_ratio}\" as a width; using the built-in {SNAP_RATIO_DEFAULT_PERCENT}%."
        ),
    }

    // Give apps time to register their windows with the OS
    thread::sleep(Duration::from_secs(2));

    let (screen_width, screen_height) =
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let (left_width, right_width) = split_widths(screen_width, width.percent());

    // Snap Open Stage Control to its share on the left
    if let Some(hwnd) = find_window_to_snap(osc_name, OPEN_STAGE_CONTROL_WINDOW_DEFAULT) {
        place_window(hwnd, 0, left_width, screen_height);
        console_log!("Open Stage Control anchored to the left ({left_width}px).");
    } else {
        // The name searched for goes in the message because it is a setting
        // now: an app that never started and a name that was mistyped look
        // exactly alike from here otherwise.
        console_log!(
            "Could not find an Open Stage Control window matching \"{osc_name}\" to snap."
        );
    }

    // Snap Gig Performer to what is left of the screen
    if let Some(hwnd) = find_window_to_snap(gig_name, GIG_PERFORMER_WINDOW_DEFAULT) {
        place_window(hwnd, left_width, right_width, screen_height);
        console_log!("Gig Performer anchored to the right ({right_width}px).");

        // Placed last and given the focus, so this is the window the set
        // starts on. Raising it is not quite enough on its own: without the
        // focus the first keystroke or click still goes somewhere else.
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
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

    #[test]
    fn an_empty_width_field_uses_the_built_in_share() {
        assert_eq!(snap_width(""), SnapWidth::BuiltIn);
        assert_eq!(snap_width("   "), SnapWidth::BuiltIn);
        assert_eq!(snap_width("").percent(), SNAP_RATIO_DEFAULT_PERCENT);
    }

    #[test]
    fn a_chosen_width_is_used() {
        assert_eq!(snap_width("29"), SnapWidth::Chosen(29.0));
        assert_eq!(snap_width("29").percent(), 29.0);
    }

    /// Four spellings of the same width, all of them things the field invites:
    /// its placeholder carries a `%`, and the rig's numeric keypad types a
    /// comma where the decimal point goes.
    #[test]
    fn a_width_may_carry_a_percent_sign_spaces_or_a_comma() {
        assert_eq!(snap_width("29%"), SnapWidth::Chosen(29.0));
        assert_eq!(snap_width("  29 %  "), SnapWidth::Chosen(29.0));
        assert_eq!(snap_width("28.9"), SnapWidth::Chosen(28.9));
        assert_eq!(snap_width("28,9"), SnapWidth::Chosen(28.9));
    }

    /// The typo guard: a width this small would hide Open Stage Control rather
    /// than narrow it, so it is pulled back and the console names both numbers.
    #[test]
    fn a_width_below_the_floor_is_pulled_back() {
        assert_eq!(
            snap_width("5"),
            SnapWidth::Clamped {
                asked: 5.0,
                used: SNAP_RATIO_MIN_PERCENT,
            }
        );
        assert_eq!(snap_width("-20").percent(), SNAP_RATIO_MIN_PERCENT);
    }

    #[test]
    fn a_width_above_the_ceiling_is_pulled_back() {
        assert_eq!(
            snap_width("95"),
            SnapWidth::Clamped {
                asked: 95.0,
                used: SNAP_RATIO_MAX_PERCENT,
            }
        );
    }

    /// Not the same answer as an empty field, though both end on the built-in
    /// share: only this one lets the console say the field was the problem.
    #[test]
    fn a_width_that_is_not_a_number_is_unreadable() {
        assert_eq!(snap_width("abc"), SnapWidth::Unreadable);
        assert_eq!(snap_width("%"), SnapWidth::Unreadable);
        assert_eq!(snap_width("abc").percent(), SNAP_RATIO_DEFAULT_PERCENT);
    }

    /// Rust parses both of these as floats, and `clamp` would hand back
    /// something that looks like a perfectly good width.
    #[test]
    fn infinity_and_nan_are_unreadable() {
        assert_eq!(snap_width("inf"), SnapWidth::Unreadable);
        assert_eq!(snap_width("NaN"), SnapWidth::Unreadable);
    }

    /// The screenshot the built-in share was measured from: 555px of 1920,
    /// which rounds to 557 here — two pixels, invisible from a stage.
    #[test]
    fn the_built_in_share_splits_a_1080p_screen_as_measured() {
        assert_eq!(split_widths(1920, SNAP_RATIO_DEFAULT_PERCENT), (557, 1363));
    }

    /// The painted rectangle a given outer rectangle produces — the inverse of
    /// [`outer_rect`], and what the eye actually sees on the screen.
    fn painted(outer: (i32, i32, i32, i32), insets: FrameInsets) -> (i32, i32, i32, i32) {
        let (x, y, width, height) = outer;
        (
            x + insets.left,
            y + insets.top,
            width - insets.left - insets.right,
            height - insets.top - insets.bottom,
        )
    }

    /// Borders worth trying: none at all, the symmetric one most apps have,
    /// and a lopsided one, since a caption usually has no border above it.
    fn borders() -> [FrameInsets; 3] {
        [
            FrameInsets::default(),
            FrameInsets {
                left: 8,
                top: 8,
                right: 8,
                bottom: 8,
            },
            FrameInsets {
                left: 11,
                top: 0,
                right: 9,
                bottom: 9,
            },
        ]
    }

    #[test]
    fn a_window_with_no_border_is_placed_exactly_where_asked() {
        assert_eq!(
            outer_rect(557, 0, 1363, 1080, FrameInsets::default()),
            (557, 0, 1363, 1080)
        );
    }

    /// Whatever the border, what ends up painted is the strip that was asked
    /// for. This is the whole of the gap fix.
    #[test]
    fn the_painted_edges_land_where_asked_whatever_the_border() {
        for insets in borders() {
            let outer = outer_rect(557, 0, 1363, 1080, insets);
            assert_eq!(
                painted(outer, insets),
                (557, 0, 1363, 1080),
                "a {insets:?} border moved the painted edges"
            );
        }
    }

    /// The bug the user saw: placed by their window rects alone, the two
    /// windows left two borders of whatever was behind showing between them —
    /// about 20px on this rig, through which the Open Stage Control launcher
    /// window could be read.
    #[test]
    fn the_two_windows_meet_with_nothing_showing_between() {
        let (left_width, right_width) = split_widths(1920, SNAP_RATIO_DEFAULT_PERCENT);
        for insets in borders() {
            let osc = painted(outer_rect(0, 0, left_width, 1080, insets), insets);
            let gig = painted(outer_rect(left_width, 0, right_width, 1080, insets), insets);

            assert_eq!(
                osc.0 + osc.2,
                gig.0,
                "a {insets:?} border left a gap between the two windows"
            );
            assert_eq!(osc.0, 0, "Open Stage Control missed the left screen edge");
            assert_eq!(
                gig.0 + gig.2,
                1920,
                "Gig Performer missed the right screen edge"
            );
        }
    }

    /// What halving twice used to get wrong: `screen_width / 2` on both sides
    /// left a one-pixel strip of desktop down the middle of an odd-width
    /// screen.
    #[test]
    fn the_two_widths_always_cover_the_screen() {
        for screen_width in [1920, 1921, 2560, 3441] {
            for percent in [10.0, 29.0, 50.0, 90.0] {
                let (left, right) = split_widths(screen_width, percent);
                assert_eq!(
                    left + right,
                    screen_width,
                    "{percent}% of {screen_width} left a gap"
                );
            }
        }
    }
}
