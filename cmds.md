# Build commands

The app ships as `Gig Control Center.exe`, and as two named downloads on each
release:

```
target/
├── debug/                                        cargo debug profile
└── release/
    ├── gig-center.exe                            what cargo writes
    └── rust/Gig Control Center.exe               the shipped copy

dist/
├── Gig-Control-Center-v1.0.0-portable.exe        the same exe, release name
└── Gig-Control-Center-v1.0.0-installer.exe       ISCC output
```

All of `target/` and `dist/` is git-ignored, and `cargo clean` empties
`target/`.

## Tests

```bash
cargo test
```

Covers the screen matcher against its reference implementation, the settings
merge rules, device fallback, the version string the window shows, and that
every embedded Defender template still decodes at the size it has always had.

## Build

```bash
cargo build --release && mkdir -p target/release/rust && cp "target/release/gig-center.exe" "target/release/rust/Gig Control Center.exe"
```

The copy does both jobs. Cargo cannot produce that file directly: a bin
target's name becomes the crate name, which rustc rejects for containing
spaces, and cargo always writes to the profile root — its `--artifact-dir`
flag is nightly-only.

`cargo run --release` still runs `target/release/gig-center.exe`, which reads
`settings.json` from the working directory. The copy in `release/rust/` is not
inside a directory named `release`, so it behaves like an install instead and
keeps its settings beside itself.

The build is self-contained — the UI, the icons and the screen-match templates
are all compiled in — so nothing has to be shipped alongside the exe.

## Installer

Needs Inno Setup 6 (`choco install innosetup`); CI uses the copy on the runner
image. Run it from the repository root, after a release build:

```bash
"/c/Program Files (x86)/Inno Setup 6/ISCC.exe" /DMyAppVersion=1.0.0 packaging/gig-center.iss
```

The version has to be passed in — the script refuses to compile without it,
rather than guess and stamp the wrong number on a setup. Output lands in
`dist/`.

## Release

Normally none of the above: **Actions → Release → Run workflow**, type the
version, and the workflow bumps `Cargo.toml`, tags, builds, packages and
publishes. Tagging by hand does the same:

```bash
git tag v1.1.0 && git push origin v1.1.0
```

See the Releasing section of [README.md](README.md) for what comes out.
