fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    prescale_window_icon();

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        // FileVersion and ProductVersion come from CARGO_PKG_VERSION on their
        // own. The strings a person actually reads do not: ProductName and
        // FileDescription both default to the crate name, so the v1.0.0 exe
        // called itself "gig-center" in Task Manager and in the file
        // properties dialog, and CompanyName was blank. FileDescription is
        // the one Task Manager shows, which is why it is worth setting rather
        // than leaving to the crate name.
        res.set("ProductName", "Gig Control Center");
        res.set("FileDescription", "Gig Control Center");
        res.set("CompanyName", "Merbac");
        res.set("LegalCopyright", "Copyright (c) 2026 Merbac. MIT licensed.");
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
