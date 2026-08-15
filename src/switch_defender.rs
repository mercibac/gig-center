use crate::console_log;
use crate::screen;
use enigo::{Button, Coordinate, Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings};
use std::thread;
use std::time::Duration;

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

    let _ = enigo.button(Button::Left, enigo::Direction::Click);
    let _ = enigo.key(Key::Meta, enigo::Direction::Click);
    thread::sleep(Duration::from_secs(1));
    
    // Type slowly to replicate pyautogui interval=0.1
    for c in "windows security".chars() {
        let _ = enigo.text(&c.to_string());
        thread::sleep(Duration::from_millis(100));
    }
    
    thread::sleep(Duration::from_millis(500));
    let _ = enigo.key(Key::Return, enigo::Direction::Click);

    thread::sleep(Duration::from_secs(7));

    let icon_def_1 = if option == "on" {
        "assets/defender_activate_icon.png"
    } else {
        "assets/defender_protection_icon.png"
    };

    screen::click_on_img(icon_def_1, &mut enigo);
    screen::click_on_img("assets/defender_settings_icon.png", &mut enigo);

    // 0.9, not the 0.8 default — switch_defender.py asks for the stricter
    // confidence on this one because the click target is derived by offset.
    let icon_def_3 = "assets/defender_realtime_icon.png";
    if let Some((img_x, img_y)) = screen::find_on_screen(icon_def_3, 0.9) {
        let target_x = img_x - 20;
        let target_y = if option == "on" {
            img_y + 160
        } else {
            img_y + 130
        };

        let _ = enigo.move_mouse(target_x, target_y, Coordinate::Abs);
        thread::sleep(Duration::from_millis(500));
        let _ = enigo.button(Button::Left, enigo::Direction::Click);
        console_log!(
            "Success! Clicked 20px below the image at: ({}, {})",
            target_x,
            target_y
        );
    } else {
        console_log!("Error: Image could not be found on the screen.");
    }

    thread::sleep(Duration::from_secs(3));
    let _ = enigo.key(Key::Alt, enigo::Direction::Press);
    let _ = enigo.key(Key::F4, enigo::Direction::Click);
    let _ = enigo.key(Key::Alt, enigo::Direction::Release);
}
