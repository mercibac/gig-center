# Gig Control Center

Live performance setup automation for Windows. One button gets the rig ready for
a gig — disables sleep and Defender's real-time protection, sets the volume,
switches the PC onto the phone's hotspot, launches Gig Performer and Open Stage
Control, sends the OSC url to the phone, and snaps the windows into place.
Another button tears it all back down.

Rust and Slint, building a single self-contained `.exe`. The UI, the icons and
the screen-match templates are all compiled in, so the binary needs nothing
beside it.

> This started as a Python prototype, which was ported to Rust and then split out
> to `E:\projects\gig-center-prototype` once the Rust version took over. That
> clone keeps the full history, so the Python side is still there to read.
> Comments here that mention "the Python prototype" are pointing at it.

## Structure

```
gig-center/
├── ui/
│   ├── app.slint            The UI, compiled by build.rs
│   └── splash.slint         The splash screen, re-exported through app.slint
├── assets/                  Images: window/exe icons, the splash art, and the
│                            screen-match templates the automation clicks on
├── settings.json            Runtime config: phone name, hotspot name, OSC preset path
│
├── build.rs                 Compiles ui/app.slint, embeds assets/icon.ico,
│                            pre-scales the window icon
├── src/
│   ├── main.rs              Entry point: backend selection, UI wiring
│   ├── backend.rs           Renderer and windowing backend selection
│   ├── assets.rs            The screen-match templates, compiled into the binary
│   ├── splash_art.rs        The splash artwork, compiled into the binary
│   ├── utils.rs             resource_path, volume control, timing
│   ├── settings.rs          Load/save settings.json
│   ├── console.rs           Thread-safe log buffer feeding the on-screen console
│   ├── gig_launcher.rs      The LAUNCH GIG RIG sequence
│   ├── gig_close.rs         The CLEAR DECK sequence
│   ├── connection.rs        Wi-Fi / hotspot detection
│   ├── snap.rs              Window positioning
│   ├── switch_defender.rs   Toggles Defender real-time protection
│   ├── switch_sleep.rs      Toggles sleep and monitor timeouts
│   ├── screen.rs, ncc.rs    Screen matching — finds a template image on screen
│   │                        and clicks it
│   ├── center.rs            Centres the window on the primary monitor
│   ├── midi_input.rs        MIDI input watching, behind the activity LED
│   ├── audio_input.rs       Audio output watching, behind the signal LED
│   └── app_icon.rs, first_paint.rs, startup_probe.rs
│                            Window icon, first-frame cloaking, startup instrumentation
│
├── Cargo.toml, Cargo.lock   Rust manifest (must stay at the root)
├── cmds.md                  The build command
└── target/                  All build output, git-ignored
    └── release/rust/Gig Control Center.exe
```

`build.rs` sits at the root rather than inside `src/`, which is where Cargo looks
for it by default — that is why the manifest names neither the build script nor
the binary.

## Running

```bash
cargo run --release
```

```bash
cargo test
```

Release is worth preferring for anything involving the screen matcher: it is
numeric hot code, and an unoptimised build makes it tens of times slower, which
is what made "clicking an image" feel broken in dev runs. `[profile.dev]` raises
`opt-level` for exactly that reason.

## CI

`.github/workflows/ci.yml` runs on every pull request and every push to `main`,
on Windows runners, since there is no other kind of build here. Two jobs:

| Job | What it runs |
| --- | --- |
| Rust | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` |
| Release build | `cargo build --release` |

Clippy is a hard gate, so the tree is kept at zero warnings. The toolchain is
pinned to 1.97.1 so a gate never moves on its own. A floating `stable` turned
main red on this workflow's first run, over a lint that did not exist when the
code was written. Bump the pin deliberately, as its own commit.

The release build is kept out of the quick job so a full optimised link does not
hold it up. It is also what covers the parts of the tree no unit test reaches:
`build.rs` compiles `ui/app.slint`, and the `include_bytes!` calls in
`assets.rs` and `splash_art.rs` fail the build if a template is renamed or
missing. Those two used to be checked by a test on the Python side; the build
does it now.

Both jobs carry a generous `timeout-minutes`, because a cold cache is expensive
here: clippy and the test build each compile the dependency graph in full, and
`[profile.dev.package."*"]` puts every one of those at `opt-level = 3`.
`cache-on-failure` is on for the same reason. rust-cache saves on success only
by default, so a job that times out writes no cache and the next run starts just
as cold, which is a hole it cannot climb out of on its own.

The exe reads `settings.json` from the working directory only when it sits
directly in `target/debug` or `target/release`, which is what `cargo run` gives
it — anywhere else, the shipped copy in `release/rust/` included, it keeps its
settings beside the exe.

## Packaging

```
target/release/rust/Gig Control Center.exe      cargo release build, ~11 MB
```

The exact command is in [cmds.md](cmds.md). In short: `cargo build --release`,
then copy the binary into `release/rust/` under the shipped name. Cargo cannot do
either step itself — a bin target's name becomes the crate name, and rustc
rejects spaces in it. The build is self-contained, so the exe needs nothing
beside it.

`cargo clean` empties `target/` entirely.

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
| `deactivation_state` | The protection card while real-time protection is **off** | `defender_deactivation_state.png` |
| `activation_state` | The same card while it is **on** | `defender_activation_state.png` |
| `realtime_label` | The "Real-time protection" label | `defender_realtime_label.png` |
| `manage_settings` | The "Manage settings" link | `defender_manage_settings.png` |

The first two are named for the state the image *shows*, not the step it is used
in, because the card is matched before the click: switching protection back on
has to find the card as it looks while protection is off. The files carry no
`_icon` suffix, unlike the rest of `assets/` — these are screenshots the matcher
searches the screen for, not icons anything draws.

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

A packaged build keeps its own `settings.json` next to the `.exe`. Nothing ships
bundled alongside it, so a fresh install starts from the built-in defaults and
writes the file on first save.

## Regenerating the assets

`assets/icon.ico`, `splash_logo.png` and `splash_shine.png` are committed, and
the build consumes them as they are. The Pillow scripts that generate them from
`assets/icon.png` live with the Python prototype — retuning the splash artwork
means running them there and copying the output back.
