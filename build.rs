fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    prescale_window_icon();

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        // FileVersion, ProductVersion and FileDescription already come from the
        // Cargo keys via WindowsResource::new(). ProductName does not — it
        // defaults to the crate name, and "gig-center" is not what the
        // installer, the shortcut or the file-properties dialog should say.
        res.set("ProductName", "Gig Control Center");
        if let Err(e) = res.compile() {
            eprintln!("Warning: Could not compile Windows resources: {e}");
        }
    }
}

/// Sizes Windows asks for: title bar (32) and taskbar / Alt+Tab (64).
const ICON_SIZES: [u32; 2] = [32, 64];

/// Resize the app icon to the sizes the window needs, here rather than at
/// startup. Decoding the PNG and running two Lanczos passes cost ~15ms on every
/// launch, and the result is identical every time — it only ever changes when
/// icon.png does, which is exactly when this build script reruns.
fn prescale_window_icon() {
    println!("cargo:rerun-if-changed=assets/icon.png");

    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let source = image::open("assets/icon.png")
        .expect("assets/icon.png must exist and be a readable image")
        .to_rgba8();

    for size in ICON_SIZES {
        let scaled =
            image::imageops::resize(&source, size, size, image::imageops::FilterType::Lanczos3);
        std::fs::write(format!("{out_dir}/icon_{size}.rgba"), scaled.into_raw())
            .expect("OUT_DIR is writable");
    }
}
