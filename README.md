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

## Installing

Three ways, all from the [releases page](https://github.com/mercibac/gig-center/releases).

**Scoop** — the one that keeps itself current:

```bash
scoop bucket add merbac https://github.com/mercibac/scoop-bucket
```

```bash
scoop install gig-center
```

`scoop update gig-center` from then on. Settings survive an update: Scoop
installs each version into its own directory, and the manifest persists
`settings.json` outside it.

**Installer** — `Gig-Control-Center-v<version>-installer.exe`. Installs for the
current user only, into `%LOCALAPPDATA%\Programs\Gig Control Center`, so it
never asks for administrator rights. Start-menu entry and an uninstaller.

**Portable** — `Gig-Control-Center-v<version>-portable.exe`. One file: put it
wherever you like and run it, and it keeps `settings.json` beside itself.

None of the downloads are code-signed, so SmartScreen warns the first time a
new version runs — **More info → Run anyway**.

## Structure

```
gig-center/
├── ui/
│   ├── app.slint            The UI, compiled by build.rs
│   └── splash.slint         The splash screen, re-exported through app.slint
├── assets/                  Images: window/exe icons, the splash art, and the
│                            screen-match templates the automation clicks on
├── settings.json            Runtime config, written beside the exe at runtime
│                            and git-ignored — a clone has none
│
├── build.rs                 Compiles ui/app.slint, embeds assets/icon.ico,
│                            pre-scales the window icon
├── src/
│   ├── main.rs              Entry point: backend selection, UI wiring
│   ├── backend.rs           Renderer and windowing backend selection
│   ├── assets.rs            The screen-match templates, compiled into the binary
│   ├── splash_art.rs        The splash artwork, compiled into the binary
│   ├── utils.rs             resource_path, volume control, timing
│   ├── version.rs           The version string the window shows
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
├── packaging/
│   └── gig-center.iss       Inno Setup script, compiled on every tag
├── .github/workflows/
│   ├── ci.yml               Lint, test and release-build gate
│   └── release.yml          Tag → portable exe, installer, release, Scoop
│
├── Cargo.toml, Cargo.lock   Rust manifest (must stay at the root)
├── LICENSE                  MIT
├── cmds.md                  The build and packaging commands
├── dist/                    Installer output, git-ignored
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

The Rust job carries a 90-minute `timeout-minutes`, because it is expensive
however it runs: clippy and the test build each compile the dependency graph in
full, and `[profile.dev.package."*"]` puts every one of those at `opt-level = 3`.
Measured on this workflow, it takes 59m11s from cold and 43m27s with a warm
cache, so the cache saves about a quarter of the time rather than most of it.
The 90 is therefore roughly double the warm case, not slack for a rare cold run.
The release build is cheap by comparison: 6m1s against its 30-minute limit.

`cache-on-failure` is on because rust-cache saves on success only by default, so
a job that times out writes no cache and the next run starts just as cold, which
is a hole it cannot climb out of on its own.

Merged pull-request branches delete themselves. That is the repository's
`delete_branch_on_merge` setting rather than anything in this workflow, so there
is nothing in here to find when you go looking for where it happens.

The exe reads `settings.json` from the working directory only when it sits
directly in `target/debug` or `target/release`, which is what `cargo run` gives
it — anywhere else, the shipped copy in `release/rust/` included, it keeps its
settings beside the exe.

`.github/workflows/release.yml` is the other workflow, and it runs on tags
rather than on pushes and pull requests — see [Releasing](#releasing). It is
the only thing here that holds `contents: write`; `ci.yml` stays on
`contents: read` because a gate has no business writing to the repository.

## Releasing

A release is one button. **Actions → Release → Run workflow**, type the new
version (`1.1.0`, no `v`), and `.github/workflows/release.yml` does the rest:
bumps `Cargo.toml`, commits that to `main`, pushes the tag, builds, packages
and publishes.

What lands on the [releases page](https://github.com/mercibac/gig-center/releases):

| Asset | What it is |
| --- | --- |
| `Gig-Control-Center-v1.0.0-portable.exe` | The exe on its own — copy it anywhere and run it |
| `Gig-Control-Center-v1.0.0-installer.exe` | Per-user installer, Start-menu entry, uninstaller |
| Source code (zip, tar.gz) | Attached by GitHub itself, nothing in the workflow does it |

Hyphens rather than the spaces in the app's own shipped name, because GitHub
rewrites spaces and runs of punctuation in an asset's name into dots — and the
Scoop manifest has to point at a URL somebody actually chose.

Tagging by hand does the same thing, for when the laptop is closer than the
browser. The workflow takes a `v*.*.*` tag push as well:

```bash
git tag v1.1.0 && git push origin v1.1.0
```

The tag and `Cargo.toml` have to agree, and the build job fails loudly when
they do not: the label in the window reads `Cargo.toml`, so a release that
disagrees with it advertises the wrong number.

Both ways are one workflow rather than two because a tag pushed with
`GITHUB_TOKEN` does not trigger another workflow — GitHub's own recursion
guard. Splitting them would need a personal access token purely to make
tagging fire the build.

The release build deliberately does not re-run clippy and the tests. That gate
costs 43-59 minutes and has already run on `main`, since a tag is cut from a
commit that passed it; repeating it would roughly quadruple the time to ship
without saying anything new.

### Where the version lives

`Cargo.toml` is the only place it is written. `src/version.rs` turns that into
what the bottom of the window shows, dropping a `.0` patch so `1.0.0` reads as
`V1.0` — a release carrying a real patch keeps it. The exe's Properties →
Details shows the same number, put there by `winresource` in `build.rs`.

## Packaging

```
target/release/gig-center.exe                      cargo release build, ~12 MB
target/release/rust/Gig Control Center.exe         the shipped copy, by hand
dist/Gig-Control-Center-v<version>-installer.exe   ISCC packaging/gig-center.iss
```

The commands are in [cmds.md](cmds.md). The build is self-contained — the UI,
the icons and the screen-match templates are all compiled in — so the exe
needs nothing beside it and the installer ships exactly one file.

`packaging/gig-center.iss` installs **per user**, into
`%LOCALAPPDATA%\Programs\Gig Control Center`, which is not a cosmetic choice.
The app writes `settings.json` next to its own exe and nothing in it ever
elevates, so a machine-wide install under `Program Files` would leave every
save failing — silently, because `#![windows_subsystem = "windows"]` means
there is no console for the warning to reach. The install directory must also
not be named `debug` or `release`, for the reason just above.

`settings.json` is not shipped and not removed on uninstall. A fresh install
starts from the built-in defaults, and an uninstall leaves the user's own
configuration where it is.

`cargo clean` empties `target/` entirely.


## Settings

`settings.json` lives beside the exe and is read fresh on every launch, so
edits made in the app's settings dialog take effect immediately. It is **not**
in the repository: a clone has none, and the app writes one from the built-in
defaults on the first save, exactly as a fresh install does. It was tracked
once, which shipped one machine's device names and preset paths to everyone
who cloned.

`cargo run` is the one exception. The exe then sits in `target/debug` or
`target/release`, which `config_dir()` reads as a dev run, so it takes the
working directory instead — the repository root, in practice.

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
| `window_names` | Which windows LAUNCH snaps into place — see below |
| `snap_ratio` | How much of the width Open Stage Control gets when they are snapped — see below |

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

`window_names` holds the two window titles LAUNCH searches for when it snaps
Open Stage Control into the left of the screen and Gig Performer into the rest,
set from the same **Advanced** section. These are not paths: each is
matched as a case-insensitive *part* of a window's title bar, and the first
visible window that matches wins. Empty means the name compiled into the exe,
so clearing a field restores it:

| Key | Built-in name |
| --- | --- |
| `open_stage_control` | `Open Stage Control` |
| `gig_performer` | `Keyboard_Live` |

The Gig Performer default is a gig file's name, because that is what Gig
Performer puts in its title bar once the file has loaded — so it goes stale
whenever a gig file is renamed, which is mainly what this is for. The snap runs
30s after the programs start, since there is nothing to match until loading has
finished, and it names the string it searched for when it finds nothing. That
line is the only way to tell a mistyped name from an app that never started.

`open_stage_control` also drives the window LAUNCH brings to the front before
sending the F5 that starts the Open Stage Control server — the same window, so
one name covers both.

`snap_ratio` is how wide Open Stage Control is when that snap happens, as a
percentage of the screen width, set from the same **Advanced** section. Gig
Performer takes whatever is left, so this is the only number there is: a second
field could be set to a pair that leaves a strip of desktop down the middle.
Empty means the built-in **29%**, which is the split this rig plays, so clearing
the field restores it — `50` is the half-and-half the app did before this
existed.

Both windows are placed by their *painted* edges rather than by the rectangle
Windows moves, which is bigger: a resizable window carries an invisible resize
border that the drop shadow is drawn into. Ignoring it left about 20px of
whatever was behind showing between the two, and Gig Performer 8px short of the
right-hand screen edge. Each window is also raised as it is placed, and Gig
Performer is left with the focus — Open Stage Control keeps a second, smaller
launcher window that is never snapped, and it used to end up on top.

The field is read loosely, because it is a plain text box: `29`, `29%`, `28.9`
and `28,9` all mean the same width. Anything outside **10 to 90** is pulled back
to the nearest end rather than honoured, since a width of `2` typed for `29`
would hide a window instead of narrowing it. The console names the width it
used on every LAUNCH, and says so plainly when a value was clamped or could not
be read at all — the same reasoning as the window names above, where a setting
that did not take effect must not look like an app that failed to start.

A packaged build keeps its own `settings.json` next to the `.exe`. Nothing ships
bundled alongside it, so a fresh install starts from the built-in defaults and
writes the file on first save.

## Regenerating the assets

`assets/icon.ico`, `splash_logo.png` and `splash_shine.png` are committed, and
the build consumes them as they are. The Pillow scripts that generate them from
`assets/icon.png` live with the Python prototype — retuning the splash artwork
means running them there and copying the output back.
