#![windows_subsystem = "windows"]

#[macro_use]
mod console;
mod app_icon;
mod assets;
mod audio_input;
mod backend;
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
mod splash_art;
mod startup_probe;
mod switch_defender;
mod switch_sleep;
mod utils;

use settings::{DefenderImages, Executables};
use slint::{Timer, TimerMode};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

slint::include_modules!();

/// How long the splash may hold the main window back before it is revealed
/// regardless. Only reachable if the startup work never finishes — a window
/// that never appears is worse than one that appears too early.
const SPLASH_DEADLINE: Duration = Duration::from_secs(20);

/// Logical size of the main window, mirroring preferred-width/height in
/// ui/app.slint. Needed explicitly because the splash is created first, and a
/// window that is not the first one created does not take its preferred size.
const MAIN_WIDTH: f32 = 800.0;
const MAIN_HEIGHT: f32 = 600.0;

/// How long to wait for the backend before deciding a splash is warranted.
///
/// Measured warm, the probe needs ~120ms from process start and the UI is ready
/// at ~70ms, so about 50ms of this is actually used and the splash never
/// appears. A cold boot blows straight through it, which is the case the splash
/// exists for. Nothing is on screen while this runs, so the wait costs a little
/// latency and no visible stutter.
const READY_GRACE: Duration = Duration::from_millis(100);

/// Reveal deadline for the quick path, where only a frame is being waited on.
const FIRST_FRAME_DEADLINE: Duration = Duration::from_millis(1000);

thread_local! {
    /// The splash, while it is up. Held here because the deferred work that
    /// dismisses it runs from the event loop, not from `main`'s scope, and
    /// dropping the component would close the window at the wrong moment.
    static SPLASH: RefCell<Option<SplashWindow>> = const { RefCell::new(None) };
}

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

    // Started before the window exists, so the whole of Slint's setup is time
    // the probe gets for nothing.
    let probe = backend::probe();

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
    seed_defender_images(&app, &settings.defender_images);
    seed_executables(&app, &settings.executables);
    app.set_console_text("System initialized. Awaiting command...\n".into());

    // Shared with every UI callback below. Slint runs them all on this
    // thread, so a RefCell is enough — no lock needed.
    let midi = Rc::new(RefCell::new(midi_input::MidiListener::new()));
    // Set once LAUNCH GIG RIG hands the input to Gig Performer; never cleared,
    // so nothing takes the device back until the app restarts.
    let midi_released = Rc::new(RefCell::new(false));
    let audio = Rc::new(RefCell::new(audio_input::AudioMeter::new()));

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
        // Read off the properties, like the dropdowns above, because the
        // Browse and Default buttons write them too — the three text fields are
        // the only ones the callback can carry as arguments.
        let chosen_images = app_weak_save
            .upgrade()
            .map(read_defender_images)
            .unwrap_or_default();
        let chosen_programs = app_weak_save
            .upgrade()
            .map(read_executables)
            .unwrap_or_default();

        let new_settings = settings::merge_from_form(
            stored,
            settings::FormInput {
                phone: &phone,
                hotspot: &hotspot,
                osc_path: &osc_path,
                midi_device: &chosen_device,
                audio_device: &chosen_audio,
                defender_images: chosen_images,
                executables: chosen_programs,
            },
        );

        settings::save_settings(&new_settings);
        console_log!("\nSettings successfully updated and saved to settings.json.");
        report_defender_images(&new_settings.defender_images);
        report_executables(&new_settings.executables);

        if let Some(app) = app_weak_save.upgrade() {
            app.set_phone_name(new_settings.phone_name.into());
            app.set_hotspot_name(new_settings.hotspot_name.into());
            app.set_osc_path(new_settings.osc_file_path.into());
            seed_defender_images(&app, &new_settings.defender_images);
            seed_executables(&app, &new_settings.executables);

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
            // Put the Advanced fields back to what is on disk. They are
            // two-way bound, so Browse, Default and typing all land on the
            // property straight away — without this, CANCEL would leave an
            // edit on screen that was never saved.
            let stored = settings::load_settings();
            seed_defender_images(&app, &stored.defender_images);
            seed_executables(&app, &stored.executables);
        }
    });

    // Pick a replacement for one of the Advanced paths. Runs on the UI thread,
    // where the dialog is modal to the app, and the properties it writes are
    // the same ones the save handler reads back.
    let app_weak_browse = app.as_weak();
    app.on_browse_for_path(move |slot| {
        let Some(app) = app_weak_browse.upgrade() else {
            return;
        };

        // The two program slots want an exe, the four Defender slots an image.
        let wants_program = matches!(slot.as_str(), "gig-performer" | "open-stage-control");
        let (title, filter_name, extensions): (_, _, &[&str]) = if wants_program {
            ("Choose the program to launch", "Programs", &["exe"])
        } else {
            (
                "Choose a Defender screen-match image",
                "Images",
                &["png", "bmp", "jpg", "jpeg"],
            )
        };

        let current = path_for(&app, &slot);
        let mut dialog = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter_name, extensions);

        // Open where the last choice came from, so replacing one of a set of
        // screenshots does not mean navigating back there every time.
        if let Some(parent) = Path::new(&current).parent().filter(|p| p.is_dir()) {
            dialog = dialog.set_directory(parent);
        }

        // Cancelling has to leave the field exactly as it was, so only a Some
        // writes anything.
        let Some(picked) = dialog.pick_file() else {
            return;
        };
        let picked = picked.to_string_lossy().to_string();

        match slot.as_str() {
            "deactivation-state" => {
                app.set_defender_deactivation_state_image(picked.clone().into())
            }
            "activation-state" => app.set_defender_activation_state_image(picked.clone().into()),
            "realtime-label" => app.set_defender_realtime_label_image(picked.clone().into()),
            "manage-settings" => app.set_defender_manage_settings_image(picked.clone().into()),
            "gig-performer" => app.set_executable_gig_performer(picked.clone().into()),
            "open-stage-control" => app.set_executable_open_stage_control(picked.clone().into()),
            other => {
                console_log!("Ignoring an unknown Advanced path slot: {other}");
                return;
            }
        }

        console_log!("\n{slot} set to {picked}");
        if wants_program {
            // Nothing to decode, so the only thing worth saying is whether it
            // is really there.
            if !Path::new(&picked).is_file() {
                console_log!("Warning: there is no file at that path.");
            }
            return;
        }
        match assets::inspect_custom_image(&picked) {
            Ok((width, height)) => console_log!("It reads as {width}x{height}."),
            Err(e) => console_log!("Warning: it will not load ({e}). Save anyway to keep it."),
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
            if enabled {
                "ON — the PC will power off."
            } else {
                "off."
            }
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

    // Registered before anything can deliver. The worker's pushed call is
    // queued and cannot run before the event loop, so it could not outrun this
    // — but the quick path below calls deliver directly, and that could.
    {
        let app_weak_handover = app.as_weak();
        let midi_for_handover = Rc::clone(&midi);
        let released_for_handover = Rc::clone(&midi_released);
        let audio_for_handover = Rc::clone(&audio);
        backend::on_ready(move |found: backend::Devices| {
            let Some(app) = app_weak_handover.upgrade() else {
                return;
            };
            apply_midi_devices(
                &app,
                &midi_for_handover,
                &released_for_handover,
                found.midi,
                true,
            );
            apply_audio_devices(&app, &audio_for_handover, found.audio, true);

            // Only when a splash is on screen does the main window need the
            // cloak armed for itself: it is then not the first window, so it
            // would otherwise flash white as it replaces the splash. Armed, it
            // is created out of composition, laid out, centred and painted
            // before anyone sees it. The splash is dismissed from the deferred
            // slot, which runs after that reveal, so the screen is never left
            // with nothing on it.
            if SPLASH.with(|slot| slot.borrow().is_some()) {
                first_paint::rearm();
                first_paint::run_after_first_frame(|| {
                    SPLASH.with(|slot| {
                        if let Some(splash) = slot.borrow_mut().take() {
                            let _ = splash.hide();
                        }
                    });
                });
            }

            // Sized explicitly because a window that is not the first one
            // created does not take its preferred size: measured 640x480
            // against the 800x600 app.slint asks for.
            app.window()
                .set_size(slint::LogicalSize::new(MAIN_WIDTH, MAIN_HEIGHT));
            let _ = app.show();
        });
    }

    // The whole question: did the backend finish while the UI was being built?
    // Nothing is on screen yet, so waiting here is latency, not a freeze.
    //
    // Held past this point only to keep the fallback alive on the slow path.
    let fallback = Timer::default();
    match backend::wait_for(&probe, READY_GRACE) {
        // Quick path — a warm start. No splash is ever created.
        backend::Probe::Ready(devices) => {
            backend::deliver(devices);
            first_paint::arm_deadline(FIRST_FRAME_DEADLINE);
        }

        // The worker died, so nothing will ever arrive. Starting with no
        // devices beats a splash that sits there until the deadline.
        backend::Probe::Failed => {
            console_log!("Could not list devices; starting without them.");
            backend::deliver(backend::Devices::default());
            first_paint::arm_deadline(FIRST_FRAME_DEADLINE);
        }

        // Slow path — a cold boot. The splash holds the screen until the
        // worker's pushed call lands.
        backend::Probe::Pending => {
            let splash = SplashWindow::new()?;
            if let Some(logo) = splash_art::logo() {
                splash.set_logo(logo);
            }
            if let Some(shine) = splash_art::shine() {
                splash.set_shine(shine);
            }
            splash.set_status("Preparing devices…".into());
            splash.show()?;
            SPLASH.with(|slot| *slot.borrow_mut() = Some(splash));

            // Last resort, should the worker never answer at all. Single-shot,
            // deliberately: a repeating timer here is exactly what spammed the
            // console before, and deliver's one-shot slot makes this harmless
            // if the devices have already arrived.
            fallback.start(TimerMode::SingleShot, SPLASH_DEADLINE, || {
                backend::deliver(backend::Devices::default());
            });

            first_paint::arm_deadline(SPLASH_DEADLINE);
        }
    }

    slint::run_event_loop()
}

/// Push the stored image paths into the Advanced fields.
///
/// Called at startup, after a save, and whenever the modal opens — the last of
/// those is what makes CANCEL discard an edit, since these fields are two-way
/// bound and so are written the moment they are touched.
fn seed_defender_images(app: &MainWindow, images: &DefenderImages) {
    app.set_defender_deactivation_state_image(images.deactivation_state.clone().into());
    app.set_defender_activation_state_image(images.activation_state.clone().into());
    app.set_defender_realtime_label_image(images.realtime_label.clone().into());
    app.set_defender_manage_settings_image(images.manage_settings.clone().into());
}

/// Read the Advanced fields back out, for the save.
fn read_defender_images(app: MainWindow) -> DefenderImages {
    DefenderImages {
        deactivation_state: app.get_defender_deactivation_state_image().to_string(),
        activation_state: app.get_defender_activation_state_image().to_string(),
        realtime_label: app.get_defender_realtime_label_image().to_string(),
        manage_settings: app.get_defender_manage_settings_image().to_string(),
    }
}

/// Push the stored program paths into the Advanced fields, like
/// [`seed_defender_images`] and for the same reasons.
fn seed_executables(app: &MainWindow, programs: &Executables) {
    app.set_executable_gig_performer(programs.gig_performer.clone().into());
    app.set_executable_open_stage_control(programs.open_stage_control.clone().into());
}

/// Read the program fields back out, for the save.
fn read_executables(app: MainWindow) -> Executables {
    Executables {
        gig_performer: app.get_executable_gig_performer().to_string(),
        open_stage_control: app.get_executable_open_stage_control().to_string(),
    }
}

/// The path currently in one Advanced field, named by the slot the UI passes.
fn path_for(app: &MainWindow, slot: &str) -> String {
    match slot {
        "deactivation-state" => app.get_defender_deactivation_state_image().to_string(),
        "activation-state" => app.get_defender_activation_state_image().to_string(),
        "realtime-label" => app.get_defender_realtime_label_image().to_string(),
        "manage-settings" => app.get_defender_manage_settings_image().to_string(),
        "gig-performer" => app.get_executable_gig_performer().to_string(),
        "open-stage-control" => app.get_executable_open_stage_control().to_string(),
        _ => String::new(),
    }
}

/// Say in the console whether each chosen program is actually there.
///
/// Same reasoning as [`report_defender_images`]: the save is not blocked, but a
/// path left behind by an uninstalled version would otherwise only announce
/// itself at LAUNCH, which is the worst moment to find out.
fn report_executables(programs: &Executables) {
    let chosen = [
        ("Gig Performer", &programs.gig_performer),
        ("Open Stage Control", &programs.open_stage_control),
    ];

    for (label, path) in chosen {
        if path.is_empty() {
            continue;
        }
        if Path::new(path).is_file() {
            console_log!("{label}: {path}");
        } else {
            console_log!("{label}: {path} is not there.");
            console_log!("The default install path will be used instead.");
        }
    }
}

/// Say in the console what each chosen image looks like, or why it will not
/// load.
///
/// The save is not blocked on this — the OSC path is not checked at save time
/// either — but a template that cannot be read clicks nothing, and finding that
/// out at the gig is the failure the embedded images were introduced to stop.
fn report_defender_images(images: &DefenderImages) {
    let chosen = [
        ("Deactivation state", &images.deactivation_state),
        ("Activation state", &images.activation_state),
        ("Realtime label", &images.realtime_label),
        ("Manage settings", &images.manage_settings),
    ];

    for (label, path) in chosen {
        if path.is_empty() {
            continue;
        }
        match assets::inspect_custom_image(path) {
            Ok((width, height)) => {
                console_log!("{label} image: {path} ({width}x{height})");
            }
            Err(e) => {
                console_log!("{label} image: {path} will not load ({e}).");
                console_log!("The built-in image will be used for that step.");
            }
        }
    }
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
    apply_midi_devices(app, midi, released, midi_input::list_devices(), announce);
}

/// The half of the above that touches the UI, given a list already in hand.
///
/// Split out so startup can enumerate on a worker thread and still end up here,
/// on the UI thread, running exactly what the settings and save paths run.
fn apply_midi_devices(
    app: &MainWindow,
    midi: &Rc<RefCell<midi_input::MidiListener>>,
    released: &Rc<RefCell<bool>>,
    devices: Vec<String>,
    announce: bool,
) {
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
    apply_audio_devices(app, audio, audio_input::list_devices(), announce);
}

/// The half of the above that touches the UI, given a list already in hand.
fn apply_audio_devices(
    app: &MainWindow,
    audio: &Rc<RefCell<audio_input::AudioMeter>>,
    devices: Vec<String>,
    announce: bool,
) {
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
