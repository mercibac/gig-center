use crate::assets::{self, DefenderSlot};
use crate::screen;
use crate::settings;
use enigo::{Button, Coordinate, Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Prevents the transient `cmd` console window from flashing on screen
/// (see the `CREATE_NO_WINDOW` process creation flag).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn open_windows_defender() {
    // `start windowsdefender:` is the CMD command that opens the Security Center,
    // mirroring what the Python prototype did, instead of UI keystrokes.
    // CREATE_NO_WINDOW keeps the cmd host hidden so nothing pops up on screen.
    match Command::new("cmd")
        .args(["/C", "start", "windowsdefender:"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
    {
        Ok(status) if status.success() => {
            console_log!("Windows Defender opened successfully.");
        }
        Ok(status) => {
            console_log!("Failed to open Windows Defender. Exit status: {status}");
        }
        Err(e) => {
            console_log!("Failed to open Windows Defender. Error: {e}");
        }
    }
}

pub fn switch_defender(option: &str) {
    console_log!("Toggling Windows Defender via UI automation...");

    let mut enigo = match Enigo::new(&EnigoSettings::default()) {
        Ok(enigo) => enigo,
        Err(e) => {
            console_log!("Error: could not take control of the keyboard/mouse ({e}).");
            console_log!("Skipping the Defender toggle — set it by hand if you need it.");
            return;
        }
    };

    open_windows_defender();

    thread::sleep(Duration::from_secs(7));

    // Read fresh here rather than taken as an argument, the way every other
    // action in this app re-reads settings.json: it keeps both call sites
    // unchanged — gig_close::main only receives two bools — and means a
    // replacement image chosen in the Advanced settings takes effect on the
    // next toggle without a restart.
    let images = settings::load_settings().defender_images;

    if option == "on" {
        // One click and done. The "Turn on" button on the protection card
        // switches real-time protection on by itself, so the two steps the way
        // off takes below — Manage settings, then the toggle under the
        // Real-time protection label — must not follow it: by then the page is
        // in its protection-on layout and that click would switch it straight
        // back off.
        let turn_on = assets::defender_template(DefenderSlot::TurnOn, &images);
        screen::click_on_img(&turn_on, &mut enigo);

        // Long enough for Defender to apply the change and redraw the card, so
        // the Alt+F4 below closes a window that has finished with it.
        thread::sleep(Duration::from_secs(5));
    } else {
        let card = assets::defender_template(DefenderSlot::ActivationState, &images);
        screen::click_on_img(&card, &mut enigo);

        let manage_settings = assets::defender_template(DefenderSlot::ManageSettings, &images);
        screen::click_on_img(&manage_settings, &mut enigo);

        // The click below is an offset from wherever this image matched, not on
        // the image itself, so a replacement has to be cropped like the original
        // one for the offset to still land on the toggle.
        let realtime_label = assets::defender_template(DefenderSlot::RealtimeLabel, &images);
        if let Some((img_x, img_y)) = screen::find_on_screen(&realtime_label, 0.8) {
            let target_x = img_x - 20;
            let target_y = img_y + 130;

            let _ = enigo.move_mouse(target_x, target_y, Coordinate::Abs);
            thread::sleep(Duration::from_millis(500));
            let _ = enigo.button(Button::Left, enigo::Direction::Click);
            console_log!(
                "Success! Clicked the toggle below the label at: ({}, {})",
                target_x,
                target_y
            );
        } else {
            console_log!("Error: Image could not be found on the screen.");
        }

        thread::sleep(Duration::from_secs(3));
    }

    let _ = enigo.key(Key::Alt, enigo::Direction::Press);
    let _ = enigo.key(Key::F4, enigo::Direction::Click);
    let _ = enigo.key(Key::Alt, enigo::Direction::Release);
}
