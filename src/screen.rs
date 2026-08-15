//! Screen-image utilities — the Rust equivalent of PyAutoGUI's image-location API.
//!
//! Provides:
//! - `find_on_screen(img_path, confidence)` → `Option<(i32, i32)>` — like `pyautogui.locateCenterOnScreen`
//! - `click_on_img(img_path, enigo)`                                — like `pyautogui.click(locateCenterOnScreen(...))`
//! - `find_img(img_path)` — shorthand with default 0.8 confidence
//!
//! Screen capture uses Win32 GDI via raw FFI — the exact same technique
//! PIL/PyAutoGUI uses on Windows (CreateDC("DISPLAY") + BitBlt).
//!
//! The match itself lives in [`crate::ncc`], which mirrors OpenCV's
//! `TM_CCOEFF_NORMED` in O(N log N) — see that module for why the previous
//! `imageproc`-based search took minutes.

use crate::console_log;
use crate::ncc;
use crate::utils::resource_path;
use enigo::{Button, Coordinate, Enigo, Mouse};
use image::GrayImage;
use std::ffi::c_void;
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

// ── Raw Win32 FFI ──────────────────────────────────────────────────────
// Using raw FFI for GDI calls avoids windows-core version conflicts
// between the `windows` and `sysinfo` crates, and matches what PIL does.

type Hdc = *mut c_void;
type Hbitmap = *mut c_void;
type Hgdiobj = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

const SRCCOPY: u32 = 0x00CC0020;
const BI_RGB: u32 = 0;
const DIB_RGB_COLORS: u32 = 0;

#[link(name = "gdi32")]
extern "system" {
    fn CreateDCA(driver: *const u8, device: *const u8, port: *const u8, pdm: *const c_void) -> Hdc;
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn CreateCompatibleBitmap(hdc: Hdc, cx: i32, cy: i32) -> Hbitmap;
    fn SelectObject(hdc: Hdc, h: Hgdiobj) -> Hgdiobj;
    fn DeleteObject(ho: Hgdiobj) -> i32;
    fn BitBlt(
        hdc: Hdc, x: i32, y: i32, cx: i32, cy: i32,
        hdc_src: Hdc, x1: i32, y1: i32, rop: u32,
    ) -> i32;
    fn GetDIBits(
        hdc: Hdc, hbm: Hbitmap, start: u32, clines: u32,
        lpv_bits: *mut c_void, lpbmi: *mut BitmapInfo, usage: u32,
    ) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn GetSystemMetrics(index: i32) -> i32;
}

// ── DPI Awareness ──────────────────────────────────────────────────────

/// Ensure the process is DPI-aware so screen captures return physical pixels
/// that match the template images.
///
/// Attempted once per process: `SetProcessDpiAwareness` returns `E_ACCESSDENIED`
/// once awareness is fixed (Slint's winit backend sets it during window
/// creation), so repeating it on every lookup only burned a syscall.
fn ensure_dpi_aware() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_SYSTEM_DPI_AWARE};
        let _ = SetProcessDpiAwareness(PROCESS_SYSTEM_DPI_AWARE);
    });
}

// ── Screen Capture ─────────────────────────────────────────────────────

/// Capture the primary monitor as a grayscale image using Win32 GDI.
///
/// Uses `CreateDC("DISPLAY") + BitBlt` — the exact same method PIL's
/// `ImageGrab.grab()` uses on Windows.
fn capture_screen() -> Option<GrayImage> {
    unsafe {
        let width = GetSystemMetrics(0); // SM_CXSCREEN
        let height = GetSystemMetrics(1); // SM_CYSCREEN

        if width <= 0 || height <= 0 {
            console_log!("Error: invalid screen dimensions {}x{}", width, height);
            return None;
        }

        // PIL/PyAutoGUI uses CreateDC("DISPLAY") — not GetDC(NULL)
        let screen_dc = CreateDCA(
            b"DISPLAY\0".as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        );
        if screen_dc.is_null() {
            console_log!("Error: CreateDC(DISPLAY) returned null.");
            return None;
        }

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            console_log!("Error: CreateCompatibleDC returned null.");
            DeleteDC(screen_dc);
            return None;
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap.is_null() {
            console_log!("Error: CreateCompatibleBitmap returned null.");
            DeleteDC(mem_dc);
            DeleteDC(screen_dc);
            return None;
        }

        let old_bitmap = SelectObject(mem_dc, bitmap);

        // BitBlt: copy the screen into our memory bitmap
        let ok = BitBlt(mem_dc, 0, 0, width, height, screen_dc, 0, 0, SRCCOPY);
        if ok == 0 {
            console_log!("Error: BitBlt(SRCCOPY) failed — screen capture not available.");
            SelectObject(mem_dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(mem_dc);
            DeleteDC(screen_dc);
            return None;
        }

        // Extract pixel data (32-bit BGRA, top-down)
        let mut bmi = BitmapInfo {
            header: BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width,
                height: -height, // negative = top-down
                planes: 1,
                bit_count: 32,
                compression: BI_RGB,
                ..Default::default()
            },
            colors: [0],
        };

        let pixel_count = (width * height) as usize;
        let mut buffer = vec![0u8; pixel_count * 4];

        let lines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            buffer.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        // Clean up GDI objects
        SelectObject(mem_dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(mem_dc);
        DeleteDC(screen_dc); // CreateDC → DeleteDC (not ReleaseDC)

        if lines == 0 {
            console_log!("Error: GetDIBits returned 0 lines.");
            return None;
        }

        // Convert BGRA → Luma8 (grayscale)
        // Standard luminance formula: Y = 0.299*R + 0.587*G + 0.114*B
        let gray_pixels: Vec<u8> = buffer
            .chunks_exact(4)
            .map(|bgra| {
                (0.299 * bgra[2] as f32 + 0.587 * bgra[1] as f32 + 0.114 * bgra[0] as f32) as u8
            })
            .collect();

        console_log!("Screen captured: {}x{}", width, height);
        GrayImage::from_raw(width as u32, height as u32, gray_pixels)
    }
}

// ── Template Matching ──────────────────────────────────────────────────

/// Locate a template image on screen and return its center coordinates.
/// This is the Rust equivalent of PyAutoGUI's `locateCenterOnScreen(img, confidence=...)`.
///
/// Scores with `TM_CCOEFF_NORMED` (via [`crate::ncc`]), the same method OpenCV
/// uses under PyAutoGUI, so `confidence` means what it means in the Python
/// parent — higher score = better match, 1.0 = pixel-perfect.
pub fn find_on_screen(img_path: &str, confidence: f32) -> Option<(i32, i32)> {
    let path = resource_path(img_path);
    if !path.exists() {
        console_log!("Error: template image not found: {:?}", path);
        return None;
    }

    let template = match image::open(&path) {
        Ok(img) => img.to_luma8(),
        Err(e) => {
            console_log!("Error loading template image {:?}: {}", path, e);
            return None;
        }
    };

    // Ensure DPI awareness before capturing the screen
    ensure_dpi_aware();

    let screen_gray = match capture_screen() {
        Some(img) => img,
        None => {
            console_log!("Error: failed to capture screen.");
            return None;
        }
    };

    console_log!(
        "Template: {}x{} ({})",
        template.width(),
        template.height(),
        img_path
    );

    // Validate template fits within the screen
    if template.width() > screen_gray.width() || template.height() > screen_gray.height() {
        console_log!(
            "Error: template image ({}x{}) is larger than the screen capture ({}x{}). \
             Possible DPI scaling mismatch.",
            template.width(),
            template.height(),
            screen_gray.width(),
            screen_gray.height()
        );
        return None;
    }

    let match_start = Instant::now();
    let best = ncc::match_ccoeff_normed(&screen_gray, &template)?;
    let match_ms = match_start.elapsed().as_secs_f64() * 1000.0;

    if best.score >= confidence {
        let center_x = (best.x + template.width() / 2) as i32;
        let center_y = (best.y + template.height() / 2) as i32;
        console_log!(
            "Icon found! {} matched at ({}, {}) with score {:.4} in {:.0} ms",
            img_path,
            center_x,
            center_y,
            best.score,
            match_ms
        );
        Some((center_x, center_y))
    } else {
        console_log!(
            "Could not find the icon {} on the screen. \
             (Best score: {:.4}, needed: {:.2}; searched in {:.0} ms)",
            img_path,
            best.score,
            confidence,
            match_ms
        );
        None
    }
}

/// Shorthand: locate a template with the default 0.8 confidence (matches PyAutoGUI default).
pub fn find_img(img_path: &str) -> Option<(i32, i32)> {
    find_on_screen(img_path, 0.8)
}

/// Click on an image found on screen — equivalent of `pyautogui.click(locateCenterOnScreen(...))`.
///
/// The 100 ms gap between the move and the press mirrors PyAutoGUI's global
/// `PAUSE = 0.1`, which it inserts after the internal `moveTo` that `click()`
/// performs. The trailing second matches the Python parent's `time.sleep(1)`.
pub fn click_on_img(img_path: &str, enigo: &mut Enigo) {
    match find_img(img_path) {
        Some((x, y)) => {
            console_log!("Clicking on {}...", img_path);
            let _ = enigo.move_mouse(x, y, Coordinate::Abs);
            thread::sleep(Duration::from_millis(100));
            let _ = enigo.button(Button::Left, enigo::Direction::Click);
            thread::sleep(Duration::from_secs(1));
        }
        None => {
            console_log!("Could not locate the image on screen: {}", img_path);
        }
    }
}
