//! Dust removal for B&W negatives (research P8).
//!
//! Dust on the film is opaque: almost no light gets through it, so on the negative it is
//! denser than any silver in the picture. Stars, catchlights and other small bright details
//! are silver, so they are never denser than the densest silver on the frame. That one
//! physical difference decides what is dust: small groups of pixels clearly denser than the
//! densest silver.
//!
//! The infrared channel cannot help on B&W film: silver blocks infrared just as dust does.

use crate::film::DustConstants;
use crate::gray::GrayFrame;
use crate::tone::raw_at;

/// Repair windows tried for each masked pixel, as half-widths in pixels.
const WINDOW_RADII: [usize; 5] = [2, 4, 8, 16, 32];

/// A window is good enough once it holds this many clean pixels.
const MIN_CLEAN: u32 = 8;

/// Remove dust specks and hairs from the raw gray `frame`, in place, given the density of the
/// densest silver on it ([`crate::tone::densest_silver`]). Returns how many were repaired.
/// Pixels outside the repaired specks (and their `dilate_px` edge) are never changed.
pub fn remove(frame: &mut GrayFrame, silver_density: f64, c: &DustConstants) -> u32 {
    let width = frame.width as usize;
    let height = frame.height as usize;
    // Raw values at or below this are denser than any silver by more than the margin.
    let limit = raw_at(silver_density + c.margin);
    let mut candidate: Vec<bool> = frame
        .pixels
        .iter()
        .map(|&v| f64::from(v) <= limit)
        .collect();

    let mut mask = vec![false; width * height];
    let mut repaired = 0;
    let mut stack = Vec::new();
    let mut component = Vec::new();
    for start in 0..candidate.len() {
        if !candidate[start] {
            continue;
        }
        // Flood fill: collect every candidate pixel connected to `start` (including
        // diagonally). An explicit stack replaces recursion: a recursive fill would make one
        // nested call per pixel, and a large dense area (thousands of pixels) would overflow
        // the thread's stack.
        component.clear();
        candidate[start] = false;
        stack.push(start);
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (usize::MAX, 0, usize::MAX, 0);
        while let Some(i) = stack.pop() {
            component.push(i);
            let (x, y) = (i % width, i / width);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            // `saturating_sub` stops at 0 instead of wrapping around at the frame's edge.
            for ny in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                    let j = ny * width + nx;
                    if candidate[j] {
                        candidate[j] = false;
                        stack.push(j);
                    }
                }
            }
        }
        let small = component.len() <= c.max_area_px as usize
            && max_x - min_x < c.max_span_px as usize
            && max_y - min_y < c.max_span_px as usize;
        if small {
            repaired += 1;
            mark_dilated(&mut mask, &component, width, height, c.dilate_px as usize);
        }
    }
    if repaired > 0 {
        fill(frame, &mask);
    }
    repaired
}

/// Mark `component` in `mask`, grown by `dilate` pixels in every direction to cover the
/// speck's soft edge.
fn mark_dilated(
    mask: &mut [bool],
    component: &[usize],
    width: usize,
    height: usize,
    dilate: usize,
) {
    for &i in component {
        let (x, y) = (i % width, i / width);
        for ny in y.saturating_sub(dilate)..=(y + dilate).min(height - 1) {
            for nx in x.saturating_sub(dilate)..=(x + dilate).min(width - 1) {
                mask[ny * width + nx] = true;
            }
        }
    }
}

/// Replace each masked pixel with the mean of the unmasked pixels in the smallest window
/// around it that holds enough of them. All new values are worked out from the unchanged
/// frame first and written afterwards, so the result does not depend on the order pixels are
/// visited in (spec FR-007).
fn fill(frame: &mut GrayFrame, mask: &[bool]) {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let mut fills = Vec::new();
    for (i, _) in mask.iter().enumerate().filter(|(_, masked)| **masked) {
        let (x, y) = (i % width, i / width);
        let mut best = None;
        for radius in WINDOW_RADII {
            let mut sum = 0u64;
            let mut count = 0u32;
            for ny in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
                let row = ny * width;
                for nx in x.saturating_sub(radius)..=(x + radius).min(width - 1) {
                    if !mask[row + nx] {
                        sum += u64::from(frame.pixels[row + nx]);
                        count += 1;
                    }
                }
            }
            if count > 0 {
                best = Some((sum + u64::from(count) / 2) / u64::from(count));
            }
            if count >= MIN_CLEAN {
                break;
            }
        }
        // With no clean pixel even in the largest window, the pixel is left as it was.
        if let Some(mean) = best {
            // A mean of u16 values always fits in a u16.
            fills.push((i, mean as u16));
        }
    }
    for (i, value) in fills {
        frame.pixels[i] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::film::{DustLevel, FilmStock};
    use crate::tone::{Histogram, auto_points, densest_silver, density};

    const W: u32 = 1200;
    const H: u32 = 800;
    const DPI: f64 = 1800.0;

    fn raw(d: f64) -> u16 {
        raw_at(d).round() as u16
    }

    /// Repeatable "grain" of ±0.05 D from a small integer hash of the position.
    fn grain(x: u32, y: u32) -> f64 {
        let hash = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 101;
        (f64::from(hash) - 50.0) / 1000.0
    }

    /// A clean picture: a 0.3–1.3 D gradient with grain.
    fn clean() -> GrayFrame {
        let mut pixels = Vec::new();
        for y in 0..H {
            for x in 0..W {
                pixels.push(raw(0.3 + f64::from(x) / f64::from(W) + grain(x, y)));
            }
        }
        GrayFrame::from_pixels(W, H, DPI, pixels)
    }

    /// A small deterministic pseudo-random sequence (a linear congruential generator).
    struct Seq(u64);
    impl Seq {
        fn next(&mut self, below: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((self.0 >> 33) % u64::from(below)) as u32
        }
    }

    /// 40 opaque specks (2–8 px across) and 10 hairs (1–2 px wide, 30–100 px long), each as
    /// its list of pixel indices, spread over the frame without touching each other.
    fn dust_objects() -> Vec<Vec<usize>> {
        let mut seq = Seq(7);
        let mut objects = Vec::new();
        for k in 0..50u32 {
            // A 10 × 5 grid of cells, one object per cell, at a random spot inside it.
            let (cx, cy) = ((k % 10) * 120 + 10, (k / 10) * 160 + 20);
            let mut pixels = Vec::new();
            if k < 40 {
                let size = 2 + seq.next(7);
                let (x0, y0) = (cx + seq.next(90), cy + seq.next(120));
                for y in y0..y0 + size {
                    for x in x0..x0 + size {
                        pixels.push((y * W + x) as usize);
                    }
                }
            } else {
                let (len, thick) = (30 + seq.next(71), 1 + seq.next(2));
                let (x0, y0) = (cx + seq.next(5), cy + seq.next(120));
                for t in 0..len {
                    for d in 0..thick {
                        // A slightly slanted hair.
                        pixels.push(((y0 + t / 4 + d) * W + x0 + t) as usize);
                    }
                }
            }
            objects.push(pixels);
        }
        objects
    }

    fn dusty(objects: &[Vec<usize>]) -> GrayFrame {
        let mut frame = clean();
        for object in objects {
            for &i in object {
                frame.pixels[i] = 30; // opaque: about 3.3 D
            }
        }
        frame
    }

    /// The densest silver on `frame`, as the processing pipeline measures it.
    fn silver(frame: &GrayFrame) -> f64 {
        let profile = FilmStock::Bw.profile();
        let hist = Histogram::from_frame(frame, profile.margin_mm);
        let black = auto_points(&hist, &profile).0.black_density;
        densest_silver(&hist, black, &profile)
    }

    fn normal() -> DustConstants {
        DustLevel::Normal.constants(DPI).unwrap()
    }

    #[test]
    fn removes_specks_and_hairs_and_nothing_else() {
        let objects = dust_objects();
        let reference = clean();
        let mut frame = dusty(&objects);
        let before = frame.clone();
        let silver = silver(&frame);
        // The 2 mm statistics margin leaves out the densest edge of the gradient.
        assert!((1.2..1.4).contains(&silver), "densest silver {silver}");
        let repaired = remove(&mut frame, silver, &normal());
        assert_eq!(repaired, 50);

        let fixed = objects
            .iter()
            .filter(|object| {
                object.iter().all(|&i| {
                    (density(frame.pixels[i]) - density(reference.pixels[i])).abs() <= 0.1
                })
            })
            .count();
        assert!(
            fixed * 100 >= 95 * objects.len(),
            "{fixed} of {}",
            objects.len()
        );

        // Every pixel more than dilate + 1 px from an object is untouched.
        let reach = normal().dilate_px as usize + 1;
        let near = |i: usize| {
            let (x, y) = (i % W as usize, i / W as usize);
            objects.iter().flatten().any(|&j| {
                let (jx, jy) = (j % W as usize, j / W as usize);
                x.abs_diff(jx) <= reach && y.abs_diff(jy) <= reach
            })
        };
        for (i, (&a, &b)) in frame.pixels.iter().zip(&before.pixels).enumerate() {
            if a != b {
                assert!(near(i), "pixel {i} changed far from any dust");
            }
        }
    }

    #[test]
    fn stars_are_kept() {
        // 100 Gaussian stars (σ 1–2 px) peaking at 1.4 D on a 0.2 D sky.
        let mut seq = Seq(11);
        let stars: Vec<(f64, f64, f64)> = (0..100)
            .map(|k| {
                let (cx, cy) = (
                    f64::from((k % 10) * 120 + 60),
                    f64::from((k / 10) * 80 + 40),
                );
                let sigma = 1.0 + f64::from(seq.next(11)) / 10.0;
                (
                    cx + f64::from(seq.next(20)),
                    cy + f64::from(seq.next(20)),
                    sigma,
                )
            })
            .collect();
        let mut pixels = Vec::new();
        for y in 0..H {
            for x in 0..W {
                let mut d: f64 = 0.2;
                for &(sx, sy, sigma) in &stars {
                    let r2 = (f64::from(x) - sx).powi(2) + (f64::from(y) - sy).powi(2);
                    if r2 < 100.0 {
                        d = d.max(0.2 + 1.2 * (-r2 / (2.0 * sigma * sigma)).exp());
                    }
                }
                pixels.push(raw(d));
            }
        }
        let mut frame = GrayFrame::from_pixels(W, H, DPI, pixels);
        let before = frame.clone();
        let silver = silver(&frame);
        remove(&mut frame, silver, &normal());
        let kept = stars
            .iter()
            .filter(|&&(sx, sy, _)| {
                let i = sy.round() as usize * W as usize + sx.round() as usize;
                frame.pixels[i] == before.pixels[i]
            })
            .count();
        assert!(kept >= 95, "{kept} of 100 stars kept");
    }

    #[test]
    fn large_dense_areas_are_left_alone() {
        let mut frame = clean();
        // A long scratch: 300 × 3 px, more than the 2 mm span and the area limit.
        for x in 100..400 {
            for y in 400..403 {
                frame.pixels[(y * W + x) as usize] = 30;
            }
        }
        let before = frame.clone();
        assert_eq!(remove(&mut frame, 1.3, &normal()), 0);
        assert_eq!(frame, before);
    }

    #[test]
    fn specks_at_the_edge_and_large_specks() {
        let mut frame = clean();
        for y in 0..3 {
            for x in 0..3 {
                frame.pixels[(y * W + x) as usize] = 30;
            }
        }
        // A 40 × 30 px speck: removed only at `strong` (0.25 mm² is 1256 px at 1800 dpi). Its
        // middle is far from clean pixels, so it is filled from the largest window.
        for y in 300..330 {
            for x in 500..540 {
                frame.pixels[(y * W + x) as usize] = 30;
            }
        }
        let mut normal_frame = frame.clone();
        assert_eq!(remove(&mut normal_frame, 1.3, &normal()), 1);
        let strong = DustLevel::Strong.constants(DPI).unwrap();
        assert_eq!(remove(&mut frame, 1.3, &strong), 2);
        let middle = (315 * W + 520) as usize;
        assert!(
            density(frame.pixels[middle]) < 1.6,
            "{}",
            density(frame.pixels[middle])
        );
        assert!(density(frame.pixels[0]) < 1.6);
    }

    #[test]
    fn stronger_levels_repair_at_least_as_much() {
        let frame = dusty(&dust_objects());
        let silver = silver(&frame);
        let count = |level: DustLevel| {
            let mut f = frame.clone();
            remove(&mut f, silver, &level.constants(DPI).unwrap())
        };
        let (gentle, normal, strong) = (
            count(DustLevel::Gentle),
            count(DustLevel::Normal),
            count(DustLevel::Strong),
        );
        assert!(
            gentle <= normal && normal <= strong,
            "{gentle} {normal} {strong}"
        );
        assert!(gentle < strong, "{gentle} {strong}");
    }

    #[test]
    fn removal_is_deterministic() {
        let objects = dust_objects();
        let mut a = dusty(&objects);
        let mut b = dusty(&objects);
        assert_eq!(
            remove(&mut a, 1.3, &normal()),
            remove(&mut b, 1.3, &normal())
        );
        assert_eq!(a, b);
    }
}
