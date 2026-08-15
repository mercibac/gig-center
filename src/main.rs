#![windows_subsystem = "windows"]

#[macro_use]
mod console;
mod app_icon;
mod connection;
mod gig_close;
mod gig_launcher;
mod ncc;
mod screen;
mod settings;
mod snap;
mod startup_probe;
mod switch_defender;
mod switch_sleep;
mod utils;

use slint::{Timer, TimerMode};
use std::thread;
use std::time::Duration;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    startup_probe::start();

    // Set explicit AppUserModelID
    unsafe {
        windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(windows::core::w!(
            "gig_control.rig.launcher.1"
        ))
        .ok();
    }

    // Configure and select the backend before any window is created.
    //
    // The software renderer is deliberate, not a fallback. Creating an OpenGL
    // context pulls in the GPU driver's ICD and costs ~570ms before the first
    // frame; drawing this UI on the CPU takes single-digit milliseconds, because
    // it is flat rectangles and text at 800x600 with no per-frame animation.
    // Trade-off: BoxShadow is a no-op in the software renderer, so the settings
    // dialog loses its drop shadow (see app.slint).
    let selector = slint::BackendSelector::new().renderer_name("software".into());
    let selector = app_icon::configure(selector);
    let selector = startup_probe::configure(selector);
    selector.select()?;
    startup_probe::mark("icon");

    let settings = settings::load_settings();
    startup_probe::mark("settings");

    let app = MainWindow::new()?;
    startup_probe::mark("window_new");
    startup_probe::watch_first_frame(app.window());

    // Initialize UI properties
    app.set_phone_name(settings.phone_name.clone().into());
    app.set_hotspot_name(settings.hotspot_name.clone().into());
    app.set_osc_path(settings.osc_file_path.clone().into());
    app.set_console_text("System initialized. Awaiting command...\n".into());

    let app_weak = app.as_weak();

    // Set up console polling timer (100ms)
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        if let Some(app) = app_weak.upgrade() {
            // Drain console logs and append
            let logs = console::drain();
            if !logs.is_empty() {
                let mut current: String = app.get_console_text().into();
                for log in logs {
                    current.push_str(&log);
                }
                app.set_console_text(current.into());
            }

            // Check if buttons need unlocking
            if console::should_unlock() {
                app.set_launch_text("LAUNCH GIG RIG".into());
                app.set_clear_text("CLEAR DECK".into());
                app.set_buttons_enabled(true);
                
                // Also log ready
                let mut current: String = app.get_console_text().into();
                current.push_str("\n--- SYSTEM READY ---\n");
                app.set_console_text(current.into());
            }
        }
    });

    let app_weak_launch = app.as_weak();
    app.on_launch_clicked(move || {
        if let Some(app) = app_weak_launch.upgrade() {
            app.set_launch_text("INITIALIZING...".into());
            app.set_clear_text("WORKING...".into());
            app.set_buttons_enabled(false);
            app.set_console_text("".into()); // Clear console
        }
        let settings = settings::load_settings();
        thread::spawn(move || {
            console::run_guarded("LAUNCH", || gig_launcher::main(settings));
        });
    });

    let app_weak_clear = app.as_weak();
    app.on_clear_clicked(move || {
        if let Some(app) = app_weak_clear.upgrade() {
            app.set_launch_text("CLEARING...".into());
            app.set_clear_text("WORKING...".into());
            app.set_buttons_enabled(false);
            app.set_console_text("".into()); // Clear console
        }
        thread::spawn(move || {
            console::run_guarded("CLEAR DECK", gig_close::main);
        });
    });

    let app_weak_save = app.as_weak();
    app.on_save_clicked(move |phone, hotspot, osc_path| {
        let phone = phone.to_string();
        let hotspot = hotspot.to_string();
        let osc_path = osc_path.to_string();

        if phone.trim().is_empty() || hotspot.trim().is_empty() {
            console_log!("Warning: Phone Name and Hotspot Name are mandatory.");
            return;
        }

        let new_settings = settings::Settings {
            phone_name: phone.trim().to_string(),
            hotspot_name: hotspot.trim().to_string(),
            osc_file_path: osc_path.trim().to_string(),
        };

        settings::save_settings(&new_settings);
        console_log!("\nSettings successfully updated and saved to settings.json.");

        if let Some(app) = app_weak_save.upgrade() {
            app.set_phone_name(new_settings.phone_name.into());
            app.set_hotspot_name(new_settings.hotspot_name.into());
            app.set_osc_path(new_settings.osc_file_path.into());
            app.set_show_settings(false);
        }
    });

    startup_probe::mark("ui_init");
    app.run()
}
