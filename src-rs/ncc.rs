//! Exact normalized cross-correlation — the O(N log N) equivalent of OpenCV's
//! `cv2.matchTemplate(..., cv2.TM_CCOEFF_NORMED)`.
//!
//! # Why this module exists
//!
//! PyAutoGUI's `locateCenterOnScreen(img, confidence=…)` hands the actual search
//! to OpenCV, which computes the correlation numerator with an FFT and the
//! per-window mean/variance terms with summed-area tables. A full-screen lookup
//! costs a few tens of milliseconds there.
//!
//! The first Rust port called `imageproc::match_template`, whose `score_at`
//! re-reads the entire template — plus the window's sum of squares — at *every*
//! output position. That is `O(screen_area × template_area)`. For a 1536×864
//! screen and the 245×237 `defender_deactivation_state.png` that is
//! 1292 × 628 = 811 376 positions × 58 065 template pixels × 2 accumulators
//! ≈ **94 billion** floating-point operations for a single lookup, and
//! `switch_defender` performs three lookups per call. That, not any of the
//! deliberate `sleep`s, is why clicking an image took so long.
//!
//! This module restores OpenCV's formulation:
//!
//! ```text
//!                    Σ(T − T̄)(I − Ī)                Σ T·I − T̄·ΣI
//! score(x, y) = ──────────────────────────── = ────────────────────────────────
//!               √( Σ(T − T̄)² · Σ(I − Ī)² )    √( (ΣT² − nT̄²)(ΣI² − (ΣI)²/n) )
//! ```
//!
//! * `Σ T·I` for every position at once → one FFT cross-correlation, O(N log N).
//! * `ΣI` and `ΣI²` over an arbitrary window → two summed-area tables, O(1) each.
//!
//! Both halves are exact, so the scores agree with the Python parent's to within
//! f64 rounding — unlike `imageproc`'s `CrossCorrelationNormalized`, which is
//! OpenCV's `TM_CCORR_NORMED` (no mean subtraction) and scores unrelated bright
//! regions deceptively high.

use image::GrayImage;
use rustfft::num_complex::Complex64;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// The best-scoring template position found by [`match_ccoeff_normed`].
pub struct Match {
    /// X of the match's top-left corner.
    pub x: u32,
    /// Y of the match's top-left corner.
    pub y: u32,
    /// `TM_CCOEFF_NORMED` score in `[-1.0, 1.0]`; 1.0 is a pixel-perfect match.
    pub score: f32,
}

/// Locate the single best `TM_CCOEFF_NORMED` match of `template` within `image`.
///
/// Returns `None` if the template is empty, larger than the image, or perfectly
/// flat (a constant template has zero variance and correlates with everything,
/// which is what OpenCV reports as an undefined match).
pub fn match_ccoeff_normed(image: &GrayImage, template: &GrayImage) -> Option<Match> {
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    let (tw, th) = (template.width() as usize, template.height() as usize);

    if tw == 0 || th == 0 || tw > iw || th > ih {
        return None;
    }

    let n = (tw * th) as f64;

    // Template terms are constant across every position, so hoist them out.
    let mut t_sum = 0u64;
    let mut t_sq_sum = 0u64;
    for &p in template.as_raw() {
        t_sum += p as u64;
        t_sq_sum += (p as u64) * (p as u64);
    }
    let t_mean = t_sum as f64 / n;
    // Σ(T − T̄)² = ΣT² − n·T̄²
    let t_var = t_sq_sum as f64 - n * t_mean * t_mean;
    if t_var <= 0.0 {
        return None;
    }

    let corr = cross_correlate(image, template);
    let (sum, sq_sum) = integral_images(image);

    let out_w = iw - tw + 1;
    let out_h = ih - th + 1;
    let stride = iw + 1;

    let mut best = Match {
        x: 0,
        y: 0,
        score: f32::MIN,
    };

    for y in 0..out_h {
        let top = y * stride;
        let bottom = (y + th) * stride;
        for x in 0..out_w {
            // Summed-area lookup. Grouped as (A + D) − (B + C) because every
            // term is a u64 and the regrouped form can never go negative.
            let win_sum =
                ((sum[bottom + x + tw] + sum[top + x]) - (sum[bottom + x] + sum[top + x + tw])) as f64;
            let win_sq = ((sq_sum[bottom + x + tw] + sq_sum[top + x])
                - (sq_sum[bottom + x] + sq_sum[top + x + tw])) as f64;

            // Σ(T − T̄)(I − Ī) collapses to Σ T·I − T̄·ΣI because Σ(T − T̄) = 0.
            let numerator = corr[y * out_w + x] - t_mean * win_sum;
            // Σ(I − Ī)² = ΣI² − (ΣI)²/n
            let i_var = win_sq - win_sum * win_sum / n;

            let score = if i_var > 0.0 {
                (numerator / (t_var * i_var).sqrt()) as f32
            } else {
                // Flat window: no variance to correlate against.
                0.0
            };

            if score > best.score {
                best = Match {
                    x: x as u32,
                    y: y as u32,
                    score,
                };
            }
        }
    }

    Some(best)
}

/// Summed-area tables of `image` and of `image²`, both `(w+1) × (h+1)` with a
/// zero first row/column so window queries need no bounds checks.
///
/// `u64` is exact and cannot overflow here: a 1536×864 screen of all-white
/// pixels sums to 3.4e8, and its squares to 8.6e10.
fn integral_images(image: &GrayImage) -> (Vec<u64>, Vec<u64>) {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let stride = w + 1;
    let mut sum = vec![0u64; stride * (h + 1)];
    let mut sq_sum = vec![0u64; stride * (h + 1)];
    let raw = image.as_raw();

    for y in 0..h {
        let row = y * w;
        let prev = y * stride;
        let cur = (y + 1) * stride;
        for x in 0..w {
            let v = raw[row + x] as u64;
            sum[cur + x + 1] = v + sum[prev + x + 1] + sum[cur + x] - sum[prev + x];
            sq_sum[cur + x + 1] = v * v + sq_sum[prev + x + 1] + sq_sum[cur + x] - sq_sum[prev + x];
        }
    }

    (sum, sq_sum)
}

/// Linear cross-correlation at every valid position:
/// `out[y][x] = Σᵢⱼ image[y+i][x+j] · template[i][j]`.
///
/// Evaluated as `IFFT(FFT(image) · conj(FFT(template)))` on a zero-padded grid.
/// Returns a row-major `out_w × out_h` buffer.
///
/// Needs three `Complex64` grids of the padded size — about 97 MB for a
/// 1536×864 screen — held for the ~120 ms the transform takes, then released.
/// `f64` rather than `f32` because raw correlation values reach ~3.8e9 here and
/// the numerator is a small difference of large terms.
fn cross_correlate(image: &GrayImage, template: &GrayImage) -> Vec<f64> {
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    let (tw, th) = (template.width() as usize, template.height() as usize);

    // An FFT computes *circular* correlation. Padding to at least
    // `iw + tw - 1` guarantees no template window wraps around an edge, so the
    // valid region is a true linear correlation.
    let fw = next_fast_len(iw + tw - 1);
    let fh = next_fast_len(ih + th - 1);

    let mut a = vec![Complex64::new(0.0, 0.0); fw * fh];
    let raw = image.as_raw();
    for y in 0..ih {
        for x in 0..iw {
            a[y * fw + x].re = raw[y * iw + x] as f64;
        }
    }

    let mut b = vec![Complex64::new(0.0, 0.0); fw * fh];
    let traw = template.as_raw();
    for y in 0..th {
        for x in 0..tw {
            b[y * fw + x].re = traw[y * tw + x] as f64;
        }
    }

    let mut planner = FftPlanner::new();
    let row_fwd = planner.plan_fft_forward(fw);
    let col_fwd = planner.plan_fft_forward(fh);
    let row_inv = planner.plan_fft_inverse(fw);
    let col_inv = planner.plan_fft_inverse(fh);

    // One scratch buffer for the transposes, one for rustfft's internals.
    let mut transposed = vec![Complex64::new(0.0, 0.0); fw * fh];
    let mut scratch = vec![
        Complex64::new(0.0, 0.0);
        row_fwd
            .get_inplace_scratch_len()
            .max(col_fwd.get_inplace_scratch_len())
            .max(row_inv.get_inplace_scratch_len())
            .max(col_inv.get_inplace_scratch_len())
    ];

    fft_2d(&mut a, fw, fh, &row_fwd, &col_fwd, &mut transposed, &mut scratch);
    fft_2d(&mut b, fw, fh, &row_fwd, &col_fwd, &mut transposed, &mut scratch);

    // Correlation (rather than convolution) is the conjugate product.
    for (av, bv) in a.iter_mut().zip(b.iter()) {
        *av *= bv.conj();
    }

    fft_2d(&mut a, fw, fh, &row_inv, &col_inv, &mut transposed, &mut scratch);

    // rustfft's inverse is unnormalized.
    let scale = 1.0 / (fw * fh) as f64;
    let out_w = iw - tw + 1;
    let out_h = ih - th + 1;
    let mut out = vec![0.0f64; out_w * out_h];
    for y in 0..out_h {
        for x in 0..out_w {
            out[y * out_w + x] = a[y * fw + x].re * scale;
        }
    }

    out
}

/// In-place 2D FFT of a `w × h` row-major buffer: transform every row, then
/// every column.
///
/// Columns are transformed by transposing into `scratch_buf` first — a blocked
/// transpose plus a contiguous pass is markedly faster than striding through
/// memory column by column.
fn fft_2d(
    buf: &mut [Complex64],
    w: usize,
    h: usize,
    row_fft: &Arc<dyn Fft<f64>>,
    col_fft: &Arc<dyn Fft<f64>>,
    scratch_buf: &mut [Complex64],
    fft_scratch: &mut [Complex64],
) {
    for row in buf.chunks_exact_mut(w) {
        row_fft.process_with_scratch(row, fft_scratch);
    }

    transpose(buf, scratch_buf, w, h);

    for col in scratch_buf.chunks_exact_mut(h) {
        col_fft.process_with_scratch(col, fft_scratch);
    }

    transpose(scratch_buf, buf, h, w);
}

/// Blocked transpose of a `w`-wide, `h`-tall row-major buffer into `dst`.
fn transpose(src: &[Complex64], dst: &mut [Complex64], w: usize, h: usize) {
    const BLOCK: usize = 32;
    for y0 in (0..h).step_by(BLOCK) {
        let y_end = (y0 + BLOCK).min(h);
        for x0 in (0..w).step_by(BLOCK) {
            let x_end = (x0 + BLOCK).min(w);
            for y in y0..y_end {
                for x in x0..x_end {
                    dst[x * h + y] = src[y * w + x];
                }
            }
        }
    }
}

/// Smallest length `≥ target` whose only prime factors are 2, 3 and 5 — the
/// sizes rustfft's mixed-radix planner handles fastest. Avoids falling back to
/// Bluestein's algorithm when a padded dimension lands on a large prime.
fn next_fast_len(target: usize) -> usize {
    if target <= 6 {
        return target.max(1);
    }

    let mut best = usize::MAX;
    let mut p5 = 1usize;
    while p5 < target * 2 {
        let mut p35 = p5;
        while p35 < target * 2 {
            // Lift by powers of two until we clear the target.
            let mut candidate = p35;
            while candidate < target {
                candidate *= 2;
            }
            if candidate < best {
                best = candidate;
            }
            p35 *= 3;
        }
        p5 *= 5;
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};
    use std::time::Instant;

    /// Deterministic pseudo-random fill so tests are reproducible.
    fn noise_image(w: u32, h: u32, seed: u64) -> GrayImage {
        let mut state = seed;
        GrayImage::from_fn(w, h, |_, _| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            Luma([(state >> 33) as u8])
        })
    }

    /// Direct textbook TM_CCOEFF_NORMED at one position — the ground truth.
    fn reference_score(image: &GrayImage, template: &GrayImage, x: u32, y: u32) -> f64 {
        let (tw, th) = (template.width(), template.height());
        let n = (tw * th) as f64;

        let t_mean = template.as_raw().iter().map(|&p| p as f64).sum::<f64>() / n;
        let mut i_sum = 0.0;
        for j in 0..th {
            for i in 0..tw {
                i_sum += image.get_pixel(x + i, y + j)[0] as f64;
            }
        }
        let i_mean = i_sum / n;

        let (mut num, mut tv, mut iv) = (0.0, 0.0, 0.0);
        for j in 0..th {
            for i in 0..tw {
                let t = template.get_pixel(i, j)[0] as f64 - t_mean;
                let v = image.get_pixel(x + i, y + j)[0] as f64 - i_mean;
                num += t * v;
                tv += t * t;
                iv += v * v;
            }
        }

        num / (tv * iv).sqrt()
    }

    #[test]
    fn finds_a_planted_template_exactly() {
        let mut image = noise_image(400, 300, 42);
        let template = noise_image(37, 29, 7);

        // Plant the template at a known offset.
        let (px, py) = (211u32, 164u32);
        for y in 0..template.height() {
            for x in 0..template.width() {
                image.put_pixel(px + x, py + y, *template.get_pixel(x, y));
            }
        }

        let m = match_ccoeff_normed(&image, &template).expect("should match");
        assert_eq!((m.x, m.y), (px, py), "wrong location");
        assert!(m.score > 0.999, "score too low: {}", m.score);
    }

    /// The whole point of the rewrite: identical scores to the textbook formula,
    /// which is what OpenCV (and therefore PyAutoGUI) computes.
    #[test]
    fn agrees_with_the_reference_formula() {
        let image = noise_image(120, 90, 99);
        let template = noise_image(17, 13, 5);

        let m = match_ccoeff_normed(&image, &template).expect("should match");
        let expected = reference_score(&image, &template, m.x, m.y);
        assert!(
            (m.score as f64 - expected).abs() < 1e-6,
            "fft={} reference={}",
            m.score,
            expected
        );

        // Spot-check arbitrary positions too, not just the winner.
        for &(x, y) in &[(0u32, 0u32), (13, 41), (100, 70)] {
            let corr = cross_correlate(&image, &template);
            let out_w = (image.width() - template.width() + 1) as usize;
            let (sum, sq_sum) = integral_images(&image);
            let stride = image.width() as usize + 1;
            let (tw, th) = (template.width() as usize, template.height() as usize);
            let n = (tw * th) as f64;
            let t_mean =
                template.as_raw().iter().map(|&p| p as f64).sum::<f64>() / n;
            let t_sq: f64 = template.as_raw().iter().map(|&p| (p as f64) * (p as f64)).sum();
            let t_var = t_sq - n * t_mean * t_mean;

            let (xs, ys) = (x as usize, y as usize);
            let top = ys * stride;
            let bottom = (ys + th) * stride;
            let win_sum = ((sum[bottom + xs + tw] + sum[top + xs])
                - (sum[bottom + xs] + sum[top + xs + tw])) as f64;
            let win_sq = ((sq_sum[bottom + xs + tw] + sq_sum[top + xs])
                - (sq_sum[bottom + xs] + sq_sum[top + xs + tw])) as f64;
            let got = (corr[ys * out_w + xs] - t_mean * win_sum)
                / (t_var * (win_sq - win_sum * win_sum / n)).sqrt();

            let expected = reference_score(&image, &template, x, y);
            assert!(
                (got - expected).abs() < 1e-9,
                "at ({}, {}): got={} reference={}",
                x,
                y,
                got,
                expected
            );
        }
    }

    #[test]
    fn rejects_degenerate_inputs() {
        let image = noise_image(50, 50, 1);
        // Template larger than the image.
        assert!(match_ccoeff_normed(&image, &noise_image(60, 10, 2)).is_none());
        // Perfectly flat template has no variance to correlate.
        let flat = GrayImage::from_pixel(10, 10, Luma([128]));
        assert!(match_ccoeff_normed(&image, &flat).is_none());
    }

    /// The regression guard for the reported bug. Uses the real asset at real
    /// screen dimensions; `imageproc` needed ~94 billion ops for this.
    #[test]
    fn full_screen_search_is_fast() {
        let template = crate::assets::defender_template(
            crate::assets::DefenderSlot::DeactivationState,
            &Default::default(),
        )
        .to_luma8()
        .expect("embedded asset should decode");
        let mut screen = noise_image(1536, 864, 2024);

        let (px, py) = (900u32, 400u32);
        for y in 0..template.height() {
            for x in 0..template.width() {
                screen.put_pixel(px + x, py + y, *template.get_pixel(x, y));
            }
        }

        let start = Instant::now();
        let m = match_ccoeff_normed(&screen, &template).expect("should match");
        let elapsed = start.elapsed();

        println!(
            "1536x864 screen vs {}x{} template: {:.1} ms (score {:.4})",
            template.width(),
            template.height(),
            elapsed.as_secs_f64() * 1000.0,
            m.score
        );

        assert_eq!((m.x, m.y), (px, py));
        assert!(m.score > 0.999, "score too low: {}", m.score);
        assert!(
            elapsed.as_secs_f64() < 2.0,
            "full-screen search took {:?} — the O(N log N) path has regressed",
            elapsed
        );
    }

    /// Side-by-side timing against the brute-force matcher this replaced.
    /// Ignored by default because the old path is slow enough to be painful:
    ///
    /// ```text
    /// cargo test --release -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "times the slow brute-force matcher on purpose"]
    fn compare_against_brute_force() {
        use imageproc::template_matching::{match_template, MatchTemplateMethod};

        let template = crate::assets::defender_template(
            crate::assets::DefenderSlot::DeactivationState,
            &Default::default(),
        )
        .to_luma8()
        .expect("embedded asset should decode");
        let mut screen = noise_image(1536, 864, 2024);
        let (px, py) = (900u32, 400u32);
        for y in 0..template.height() {
            for x in 0..template.width() {
                screen.put_pixel(px + x, py + y, *template.get_pixel(x, y));
            }
        }

        let start = Instant::now();
        let fast = match_ccoeff_normed(&screen, &template).unwrap();
        let fast_ms = start.elapsed().as_secs_f64() * 1000.0;

        let start = Instant::now();
        let slow = match_template(
            &screen,
            &template,
            MatchTemplateMethod::CrossCorrelationNormalized,
        );
        let slow_ms = start.elapsed().as_secs_f64() * 1000.0;

        let mut slow_max = f32::MIN;
        for p in slow.pixels() {
            if p[0] > slow_max {
                slow_max = p[0];
            }
        }

        println!(
            "\n  1536x864 screen, {}x{} template\n  \
             FFT + integral (TM_CCOEFF_NORMED): {:>10.1} ms  score {:.4}\n  \
             imageproc brute force (CCORR):     {:>10.1} ms  score {:.4}\n  \
             speedup: {:.0}x\n",
            template.width(),
            template.height(),
            fast_ms,
            fast.score,
            slow_ms,
            slow_max,
            slow_ms / fast_ms
        );

        assert!(slow_ms > fast_ms);
    }
}
