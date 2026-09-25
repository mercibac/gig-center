# Build commands

Both versions ship as `Gig Control Center.exe`, and both land under `target/`:

```
target/
├── debug/                              cargo debug profile
├── pyinstaller/                        pyinstaller scratch (work dir)
└── release/
    ├── python/Gig Control Center.exe   pyinstaller onefile
    └── rust/Gig Control Center.exe     cargo release build
```

All of `target/` is git-ignored. Note that `cargo clean` empties the whole tree,
the Python build included.

## Tests

```bash
uv run --with pytest pytest src-py/tests
```

Covers the MIDI device fallback, the activity filter behind the LED, the
settings round-trip, and the CLEAR DECK shutdown ordering. Every destructive
step is stubbed, so the suite never kills an app or powers the machine off.

```bash
cargo test
```

## Rust — cargo

```bash
cargo build --release && mkdir -p target/release/rust && cp "target/release/gig-center.exe" "target/release/rust/Gig Control Center.exe"
```

The copy does both jobs. Cargo cannot produce that file directly: a bin target's
name becomes the crate name, which rustc rejects for containing spaces, and
cargo always writes to the profile root — its `--artifact-dir` flag is
nightly-only.

`cargo run --release` still runs `target/release/gig-center.exe`, which reads
`settings.json` from the working directory. The copy in `release/rust/` is not
inside a directory named `release`, so it behaves like an install instead and
keeps its settings beside itself.

## Python — pyinstaller

Everything the app loads at runtime has to be listed here; a missing entry only
shows up when you launch the exe.

```bash
uv run --with pyinstaller python -m PyInstaller --noconfirm --onefile --windowed --name "Gig Control Center" --icon="assets/icon.ico" --distpath "target/release/python" --workpath "target/pyinstaller" --add-data "ui/app.slint;./ui" --add-data "ui/splash.slint;./ui" --add-data "assets/splash_logo.png;./assets" --add-data "assets/splash_shine.png;./assets" --add-data "assets/osc_icon.png;./assets" --add-data "assets/icon.ico;./assets" --add-data "assets/defender_activate_icon.png;./assets" --add-data "assets/defender_protection_icon.png;./assets" --add-data "assets/defender_realtime_icon.png;./assets" --add-data "assets/defender_settings_icon.png;./assets" --add-data "settings.json;." src-py/main.py && rm -f "Gig Control Center.spec"
```

pyinstaller is not a project dependency, so `--with` pulls it in for the run,
and `python -m` is needed because uv does not expose its console script.

Do not add `--specpath`: pyinstaller resolves relative `--add-data` sources
against the spec file's directory, so moving the spec silently breaks every one
of them. The generated spec is written to the repository root instead and
deleted above — this command line regenerates it every run.

## Python — nuitka (alternative, not the shipped build)

Produces a folder of DLLs beside the exe rather than a single file. Kept for
reference; `--output-dir` keeps it out of the source tree. In a non-interactive
shell add `--disable-ccache`, or nuitka stops to ask whether it may download
ccache.

```bash
uv run --with nuitka python -m nuitka --standalone --output-dir="target/nuitka" --windows-disable-console --output-filename="Gig Control Center.exe" --windows-icon-from-ico="assets/icon.ico" --include-data-files="ui/app.slint=ui/app.slint" --include-data-files="ui/splash.slint=ui/splash.slint" --include-data-files="assets/splash_logo.png=assets/splash_logo.png" --include-data-files="assets/splash_shine.png=assets/splash_shine.png" --include-data-files="assets/icon.ico=assets/icon.ico" --include-data-files="settings.json=settings.json" --include-data-files="assets/osc_icon.png=assets/osc_icon.png" --include-data-files="assets/defender_activate_icon.png=assets/defender_activate_icon.png" --include-data-files="assets/defender_protection_icon.png=assets/defender_protection_icon.png" --include-data-files="assets/defender_realtime_icon.png=assets/defender_realtime_icon.png" --include-data-files="assets/defender_settings_icon.png=assets/defender_settings_icon.png" src-py/main.py
```

With upx compression on top — `--upx-binary` still needs a real path to upx:

```bash
uv run --with nuitka python -m nuitka --standalone --output-dir="target/nuitka" --follow-imports --deployment --lto=yes --windows-disable-console --output-filename="Gig Control Center.exe" --windows-icon-from-ico="assets/icon.ico" --windows-company-name="Your Company" --windows-product-name="Gig Control Center" --windows-file-version="1.0.0.0" --windows-product-version="1.0.0.0" --include-data-files="ui/app.slint=ui/app.slint" --include-data-files="ui/splash.slint=ui/splash.slint" --include-data-files="assets/splash_logo.png=assets/splash_logo.png" --include-data-files="assets/splash_shine.png=assets/splash_shine.png" --include-data-files="assets/icon.ico=assets/icon.ico" --include-data-files="settings.json=settings.json" --include-data-dir="assets=assets" --plugin-enable=upx --upx-binary="C:\path\to\upx.exe" --remove-output --no-pyi-file src-py/main.py
```
