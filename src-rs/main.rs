#![windows_subsystem = "windows"]

#[macro_use]
mod console;
mod app_icon;
mod assets;
mod audio_input;
mod center;
mod connection;
mod first_paint;
mod gig_close;
mod gig_launcher;
mod midi_input;
mod ncc;
mod screen;
mod settings;
mod snap;
mod startup_probe;
mod switch_defender;
mod switch_sleep;
mod utils;

use slint::{Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    startup_probe::start();

    // Must be armed before any window exists: the window it cloaks is created
    // later, by the event loop.
    first_paint::start();

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
    let selector = first_paint::configure(selector);
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
    app.set_shutdown_after_clear(settings.shutdown_after_clear);
    app.set_manage_defender(settings.manage_defender);
    app.set_console_text("System initialized. Awaiting command...\n".into());

    // Shared with every UI callback below. Slint runs them all on this
    // thread, so a RefCell is enough — no lock needed.
    let midi = Rc::new(RefCell::new(midi_input::MidiListener::new()));
    // Set once LAUNCH GIG RIG hands the input to Gig Performer; never cleared,
    // so nothing takes the device back until the app restarts.
    let midi_released = Rc::new(RefCell::new(false));
    let audio = Rc::new(RefCell::new(audio_input::AudioMeter::new()));

    // Deferred: enumerating MIDI devices initialises the MIDI subsystem, which
    // measured ~90-105ms. Run before the event loop it blocked the first paint
    // and pushed startup from ~70ms to ~200ms, so it waits for the window.
    {
        let app_weak_devices = app.as_weak();
        let midi_for_devices = Rc::clone(&midi);
        let released_for_devices = Rc::clone(&midi_released);
        let audio_for_devices = Rc::clone(&audio);
        first_paint::run_after_first_frame(move || {
            if let Some(app) = app_weak_devices.upgrade() {
                refresh_midi_devices(&app, &midi_for_devices, &released_for_devices, true);
                refresh_audio_devices(&app, &audio_for_devices, true);
                // Everything deferred is done, so the UI can come out from
                // behind its veil. Last, deliberately: readiness means the
                // work above finished, not that it started.
                app.set_ready(true);
            }
        });
    }

    // Faster than the console timer: the LED is a flicker, not a readout.
    // midir's callback runs on its own thread, so polling from a Slint timer
    // is what keeps the UI property on the UI thread.
    let midi_for_timer = Rc::clone(&midi);
    let audio_for_timer = Rc::clone(&audio);
    let app_weak_midi = app.as_weak();
    let midi_timer = Timer::default();
    midi_timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        if let Some(app) = app_weak_midi.upgrade() {
            app.set_midi_active(midi_for_timer.borrow().is_active());
            app.set_audio_active(audio_for_timer.borrow().is_active());
        }
    });

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
    let midi_for_launch = Rc::clone(&midi);
    let released_for_launch = Rc::clone(&midi_released);
    app.on_launch_clicked(move || {
        if let Some(app) = app_weak_launch.upgrade() {
            app.set_launch_text("INITIALIZING...".into());
            app.set_clear_text("WORKING...".into());
            app.set_buttons_enabled(false);
            app.set_console_text("".into()); // Clear console
        }
        // Before anything launches, so the port is free the moment Gig
        // Performer looks for it.
        release_midi(&app_weak_launch, &midi_for_launch, &released_for_launch);

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
        let settings = settings::load_settings();
        thread::spawn(move || {
            console::run_guarded("CLEAR DECK", || {
                gig_close::main(settings.shutdown_after_clear, settings.manage_defender)
            });
        });
    });

    let app_weak_save = app.as_weak();
    let midi_for_save = Rc::clone(&midi);
    let audio_for_save = Rc::clone(&audio);
    let released_for_save = Rc::clone(&midi_released);
    app.on_save_clicked(move |phone, hotspot, osc_path| {
        let phone = phone.to_string();
        let hotspot = hotspot.to_string();
        let osc_path = osc_path.to_string();

        if phone.trim().is_empty() || hotspot.trim().is_empty() {
            console_log!("Warning: Phone Name and Hotspot Name are mandatory.");
            return;
        }

        // Read back rather than rebuilt from scratch: the toggles and the
        // device live outside this form and would otherwise be dropped.
        let stored = settings::load_settings();
        let chosen_device = app_weak_save
            .upgrade()
            .map(|app| app.get_midi_device().to_string())
            .unwrap_or_default();
        let chosen_audio = app_weak_save
            .upgrade()
            .map(|app| app.get_audio_device().to_string())
            .unwrap_or_default();

        let new_settings = settings::merge_from_form(
            stored,
            &phone,
            &hotspot,
            &osc_path,
            &chosen_device,
            &chosen_audio,
        );

        settings::save_settings(&new_settings);
        console_log!("\nSettings successfully updated and saved to settings.json.");

        if let Some(app) = app_weak_save.upgrade() {
            app.set_phone_name(new_settings.phone_name.into());
            app.set_hotspot_name(new_settings.hotspot_name.into());
            app.set_osc_path(new_settings.osc_file_path.into());

            // Follow the devices that were just saved. Without this the meter
            // and the listener stay on whatever they opened last, so a new
            // choice only took effect on the next settings open or a restart.
            // Both helpers re-read settings.json, so this has to run after the
            // save above. refresh_midi_devices still honours the release rule,
            // so a save cannot take the port back from Gig Performer.
            refresh_midi_devices(&app, &midi_for_save, &released_for_save, false);
            refresh_audio_devices(&app, &audio_for_save, false);

            app.set_show_settings(false);
        }
    });

    // Re-enumerate when the modal opens, so a controller plugged in
    // mid-session shows up without a restart.
    let app_weak_settings = app.as_weak();
    let midi_for_settings = Rc::clone(&midi);
    let audio_for_settings = Rc::clone(&audio);
    let released_for_settings = Rc::clone(&midi_released);
    app.on_settings_opened(move || {
        if let Some(app) = app_weak_settings.upgrade() {
            refresh_midi_devices(&app, &midi_for_settings, &released_for_settings, false);
            refresh_audio_devices(&app, &audio_for_settings, false);
        }
    });

    // Both toggles live on the main screen, so there is no Save button to
    // carry them — persist the moment they are pressed.
    app.on_shutdown_toggled(move |enabled| {
        let mut settings = settings::load_settings();
        settings.shutdown_after_clear = enabled;
        settings::save_settings(&settings);
        console_log!(
            "
Shutdown after CLEAR DECK: {}",
            if enabled { "ON — the PC will power off." } else { "off." }
        );
    });

    app.on_defender_toggled(move |enabled| {
        let mut settings = settings::load_settings();
        settings.manage_defender = enabled;
        settings::save_settings(&settings);
        console_log!(
            "
Windows Defender step: {}",
            if enabled {
                "ON — LAUNCH and CLEAR DECK will toggle it."
            } else {
                "off — both will skip it."
            }
        );
    });

    startup_probe::mark("ui_init");
    first_paint::arm_deadline();
    app.run()
}

/// Re-read the connected devices and (re)open the one to listen on.
///
/// Runs at startup and every time the settings modal opens. Reports only when
/// something changed, so reopening settings does not spam the console — except
/// at startup, where `announce` forces one line so the state is never a
/// mystery. Port of GigControlApp.refresh_midi_devices.
fn refresh_midi_devices(
    app: &MainWindow,
    midi: &Rc<RefCell<midi_input::MidiListener>>,
    released: &Rc<RefCell<bool>>,
    announce: bool,
) {
    let devices = midi_input::list_devices();
    let model: Vec<slint::SharedString> = devices.iter().map(|d| d.as_str().into()).collect();
    app.set_midi_devices(slint::ModelRc::new(slint::VecModel::from(model)));

    let saved = settings::load_settings().midi_device;
    let wanted = midi_input::choose_device(&saved, &devices);
    app.set_midi_device(wanted.clone().unwrap_or_default().into());

    // Listing ports does not open them, so the dropdown above stays current
    // even mid-gig. Reopening one would put this app back on the device behind
    // the user's back, which is what release_midi exists to prevent.
    if *released.borrow() {
        return;
    }

    let mut listener = midi.borrow_mut();
    if wanted != listener.device || announce {
        match &wanted {
            Some(name) => {
                if listener.open(name) {
                    console_log!("Listening for MIDI on '{}'.", name);
                }
            }
            None => {
                listener.close();
                console_log!("No MIDI device connected.");
            }
        }
    }
}

/// Hand the MIDI input to Gig Performer.
///
/// The handover lasts for the rest of the session: nothing takes the device
/// back, not CLEAR DECK and not reopening the settings modal. Only restarting
/// the app starts listening again, so a running Gig Center can never surprise
/// Gig Performer by reclaiming the input mid-gig.
fn release_midi(
    app: &slint::Weak<MainWindow>,
    midi: &Rc<RefCell<midi_input::MidiListener>>,
    released: &Rc<RefCell<bool>>,
) {
    *released.borrow_mut() = true;
    midi.borrow_mut().close();
    if let Some(app) = app.upgrade() {
        app.set_midi_active(false);
    }
    console_log!("MIDI released for the rest of the session — restart to listen again.");
}

/// Re-read the audio outputs and (re)watch the one to meter.
///
/// Mirrors `refresh_midi_devices`, minus the release rule: nothing competes
/// with this app for a peak meter, so there is no reason to ever stop watching.
fn refresh_audio_devices(
    app: &MainWindow,
    audio: &Rc<RefCell<audio_input::AudioMeter>>,
    announce: bool,
) {
    let devices = audio_input::list_devices();
    let model: Vec<slint::SharedString> = devices.iter().map(|d| d.as_str().into()).collect();
    app.set_audio_devices(slint::ModelRc::new(slint::VecModel::from(model)));

    let saved = settings::load_settings().audio_device;
    let wanted = audio_input::choose_device(&saved, &devices);
    app.set_audio_device(wanted.clone().unwrap_or_default().into());

    let mut meter = audio.borrow_mut();
    if wanted != meter.device || announce {
        match &wanted {
            Some(name) => {
                if meter.open(name) {
                    console_log!("Watching audio on '{}'.", name);
                }
            }
            None => {
                meter.close();
                console_log!("No audio output found.");
            }
        }
    }
}
