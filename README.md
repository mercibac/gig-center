# Gig Control Center

Live performance setup automation for Windows. One button gets the rig ready for
a gig — disables sleep and Defender's real-time protection, sets the volume,
switches the PC onto the phone's hotspot, launches Gig Performer and Open Stage
Control, sends the OSC url to the phone, and snaps the windows into place.
Another button tears it all back down.

The project holds **two complete implementations** of the same app — one in
Python, one in Rust. They are independent, and either can be built and shipped
on its own. What they share is the UI definition, the image assets and the
settings file.

## Structure

```
gig-center/
├── ui/
│   └── app.slint            Shared Slint UI — both versions render this same file
├── assets/                  Shared images: window/exe icons, and the screen-match
│                            templates the automation clicks on
├── settings.json            Shared runtime config: phone name, hotspot name, OSC preset path
│
├── src-py/                  Python version
│   ├── main.py              Entry point: loads the UI, wires buttons, redirects the console
│   ├── utils.py             resource_path, settings loading, volume control, timing
│   ├── gig_launcher.py      The LAUNCH GIG RIG sequence
│   ├── gig_close.py         The CLEAR DECK sequence
│   ├── connection.py        Wi-Fi / hotspot detection
│   ├── snap.py              Window positioning
│   ├── switch_defender.py   Toggles Defender real-time protection
│   └── switch_sleep.py      Toggles sleep and monitor timeouts
│
├── src-rs/                  Rust version
│   ├── build.rs             Compiles ui/app.slint, embeds assets/icon.ico, pre-scales the window icon
│   ├── main.rs              Entry point: backend selection, UI wiring
│   ├── utils.rs             resource_path, volume control, timing
│   ├── settings.rs          Load/save settings.json
│   ├── console.rs           Thread-safe log buffer feeding the on-screen console
│   ├── gig_launcher.rs      The LAUNCH GIG RIG sequence
│   ├── gig_close.rs         The CLEAR DECK sequence
│   ├── connection.rs        Wi-Fi / hotspot detection
│   ├── snap.rs              Window positioning
│   ├── switch_defender.rs   Toggles Defender real-time protection
│   ├── switch_sleep.rs      Toggles sleep and monitor timeouts
│   ├── screen.rs, ncc.rs    Screen matching — finds a template image on screen and clicks it
│   └── app_icon.rs, first_paint.rs, startup_probe.rs
│                            Window icon, first-frame cloaking, startup instrumentation
│
├── Cargo.toml, Cargo.lock   Rust manifest (must stay at the root)
├── pyproject.toml, uv.lock  Python dependencies
└── cmds.md                  Recorded PyInstaller / Nuitka packaging commands
```

Most modules in `src-py/` have a same-named counterpart in `src-rs/`. The extras
on the Rust side are things Python gets from its libraries: `screen.rs`/`ncc.rs`
replace `pyautogui.locateCenterOnScreen`, and `console.rs` replaces the
`ConsoleRedirector` that lives inside `main.py`.

## Running

Both versions expect the repository root as the working directory — that is where
they look for `settings.json`, `ui/` and `assets/`.

Python:

```bash
uv run python src-py/main.py
```

Rust:

```bash
cargo run --release
```

Tests for the Rust screen matcher:

```bash
cargo test
```

## Packaging

The Rust version builds to a single `target/release/gig-center.exe`, with the UI
and icons compiled in — nothing to ship alongside it except `settings.json`.

The Python version is packaged with PyInstaller or Nuitka; the exact commands,
including every `--add-data` / `--include-data-files` path, are kept in
[cmds.md](cmds.md).

## Settings

`settings.json` sits at the repository root and is read fresh on every launch, so
edits made in the app's settings dialog take effect immediately:

| Key | Meaning |
| --- | --- |
| `phone_name` | KDE Connect device name of the phone, used to send it the OSC url |
| `hotspot_name` | SSID the PC switches to before launching anything |
| `osc_file_path` | Open Stage Control preset to load |

In a packaged build the file lives next to the `.exe` instead.
