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
│   ├── switch_sleep.py      Toggles sleep and monitor timeouts
│   ├── midi_input.py        MIDI input watching, behind the activity LED
│   ├── window_setup.py      Title bar icon and centring — Win32, since Slint's
│   │                        Python binding exposes neither
│   └── tests/               pytest suite (see cmds.md)
│
├── src-rs/                  Rust version
│   ├── build.rs             Compiles ui/app.slint, embeds assets/icon.ico, pre-scales the window icon
│   ├── main.rs              Entry point: backend selection, UI wiring
│   ├── assets.rs            The screen-match templates, compiled into the binary
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
│   ├── center.rs            Centres the window on the primary monitor
│   ├── midi_input.rs        MIDI input watching, behind the activity LED
│   └── app_icon.rs, first_paint.rs, startup_probe.rs
│                            Window icon, first-frame cloaking, startup instrumentation
│
├── Cargo.toml, Cargo.lock   Rust manifest (must stay at the root)
├── pyproject.toml, uv.lock  Python dependencies
├── cmds.md                  The build commands for both versions
└── target/                  All build output, git-ignored
    └── release/
        ├── python/Gig Control Center.exe
        └── rust/Gig Control Center.exe
```

Both versions carry the same features: the MIDI activity LED and device
picker, the Defender button that skips the `switch_defender` step, and the
shutdown-after-CLEAR-DECK toggle.

Most modules in `src-py/` have a same-named counterpart in `src-rs/`. The extras
on the Rust side are things Python gets from its libraries: `screen.rs`/`ncc.rs`
replace `pyautogui.locateCenterOnScreen`, and `console.rs` replaces the
`ConsoleRedirector` that lives inside `main.py`.

## Running

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

The Python entry point resolves `ui/`, `assets/` and `settings.json` relative to
its own location, so it runs from any working directory. The Rust build reads
`settings.json` from the working directory only when the exe sits directly in
`target/debug` or `target/release`, which is what `cargo run` gives it —
anywhere else, the shipped copy in `release/rust/` included, it keeps its
settings beside the exe.

## Packaging

Both versions ship as `Gig Control Center.exe`, side by side under `target/`,
which is git-ignored in full:

```
target/release/python/Gig Control Center.exe    pyinstaller onefile, ~67 MB
target/release/rust/Gig Control Center.exe      cargo release build, ~11 MB
```

The exact commands are in [cmds.md](cmds.md). In short:

**Rust** — `cargo build --release`, then copy the binary into `release/rust/`
under the shipped name. Cargo cannot do either step itself: a bin target's name
becomes the crate name, and rustc rejects spaces in it. The build is
self-contained — the UI, the icons and the screen-match templates are compiled
in — so the exe needs nothing beside it.

**Python** — PyInstaller, one self-extracting file. Nothing is compiled in, so
`ui/app.slint`, the `assets/` images and `settings.json` all have to be listed
on the command line; miss one and the build only fails when you launch it. A
Nuitka recipe is kept in cmds.md as an alternative, but PyInstaller is what
produces the layout above.

`cargo clean` empties `target/` entirely — the Python build with it.

## Settings

`settings.json` sits at the repository root and is read fresh on every launch, so
edits made in the app's settings dialog take effect immediately:

| Key | Meaning |
| --- | --- |
| `phone_name` | KDE Connect device name of the phone, used to send it the OSC url |
| `hotspot_name` | SSID the PC switches to before launching anything |
| `osc_file_path` | Open Stage Control preset to load |
| `midi_device` | MIDI input to watch; falls back to the first connected device when absent |
| `audio_device` | Audio output to watch for the signal LED; same fallback as `midi_device` |
| `shutdown_after_clear` | Power the PC off 2s after CLEAR DECK finishes (default off) |
| `manage_defender` | Whether LAUNCH and CLEAR DECK toggle Defender at all (default on) |
| `defender_images` | Replacements for the four Defender screen-match images — see below |
| `executables` | Where Gig Performer and Open Stage Control live — see below |

`defender_images` holds one path per Defender step, set from **Advanced** in the
settings dialog. An empty entry, which is the default, means the image compiled
into the exe from `assets/`, so clearing a field is how you get the original
back. A path that cannot be read or decoded falls back to the built-in image and
says so in the console, rather than leaving the automation clicking nothing.

| Key | What the image shows | Bundled file |
| --- | --- | --- |
| `deactivation_state` | The protection card while real-time protection is **off** | `defender_activate_icon.png` |
| `activation_state` | The same card while it is **on** | `defender_protection_icon.png` |
| `realtime_label` | The "Real-time protection" label | `defender_realtime_icon.png` |
| `manage_settings` | The "Manage settings" link | `defender_settings_icon.png` |

The first two are named for the state the image *shows*, not the step it is used
in, because the card is matched before the click: switching protection back on
has to find the card as it looks while protection is off. The bundled filenames
still carry the older wording — they are shared with the Python build, so
renaming them is a separate job.

Only the Rust build acts on `defender_images`. The Python build shows the
Advanced fields but its Browse buttons do nothing, and it neither reads the
paths nor applies them to its own Defender step. It does keep them: it loads
`settings.json` whole and writes it back whole, so saving from the Python build
leaves the block intact rather than clearing it.

One caveat for `realtime_label`: that step clicks a fixed offset below wherever
the image matched rather than the image itself, so a replacement has to be
cropped the same way as the original for the offset to still land on the toggle.

`executables` holds the two programs LAUNCH starts — `gig_performer` and
`open_stage_control` — set from the same **Advanced** section. Empty means the
install path compiled into the exe, so clearing a field restores it:

| Program | Built-in path |
| --- | --- |
| `gig_performer` | `C:\Program Files\Deskew Technologies\Gig Performer 5\GigPerformer5.exe` |
| `open_stage_control` | `D:\Apps\portable\open-stage-control_1.30.1_win32-x64\open-stage-control.exe` |

The Open Stage Control default carries its version in the folder name, so it
goes stale on every upgrade — that is mainly what this is for. A path that is
no longer there falls back to the built-in one and says so in the console,
rather than starting nothing; LAUNCH also logs which executable it used.

A packaged build keeps its own copy next to the `.exe`. The Python builds ship
the repository's `settings.json` inside the bundle and seed that copy from it —
on a fresh install, and again if the file is ever corrupted — so an install
starts with working values rather than placeholders. The Rust build carries no
bundled copy and starts from its built-in defaults.
