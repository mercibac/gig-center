# Build commands

The app ships as `Gig Control Center.exe` under `target/`:

```
target/
├── debug/                              cargo debug profile
└── release/
    ├── gig-center.exe                  what cargo writes
    └── rust/Gig Control Center.exe     the shipped copy
```

All of `target/` is git-ignored, and `cargo clean` empties it.

## Tests

```bash
cargo test
```

Covers the screen matcher against its reference implementation, the settings
merge rules, device fallback, and that every embedded Defender template still
decodes at the size it has always had.

## Build

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

The build is self-contained — the UI, the icons and the screen-match templates
are all compiled in — so nothing has to be shipped alongside the exe.
