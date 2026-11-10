//! Turning negative densities into positive tones (research P4, P5).
//!
//! Film density is close to linear in the logarithm of the exposure, so equal steps in density
//! look like equal steps in the print. Each frame's densest and least dense picture areas set
//! the white and black points; every tone step depends only on a pixel's raw value, so the
//! whole mapping is one 65,536-entry lookup table applied to every pixel.
//!
//! There is no brightness, contrast or matching between frames: those are set in post
//! (research P15, P16).

use serde::Serialize;

use crate::film::Profile;
use crate::gray::GrayFrame;

/// Full scale of a 16-bit sample.
const FULL: f64 = 65535.0;

/// Density of a raw value relative to full-scale light: 0 for the brightest light, about 2 for
/// dense silver. Zero is treated as 1 so the logarithm stays finite.
pub fn density(raw: u16) -> f64 {
    (FULL / f64::from(raw.max(1))).log10()
}

/// The raw value at which density reaches `d` (the inverse of [`density`]).
pub fn raw_at(d: f64) -> f64 {
    FULL * 10f64.powf(-d)
}

/// Where a frame's black and white points came from (data-model "Tone Points").
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointSource {
    /// Measured on this frame.
    Auto,
    /// The frame is probably blank; a nominal range is used.
    BlankDefault,
}

/// The densities shown as black and white.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TonePoints {
    pub black_density: f64,
    pub white_density: f64,
    pub source: PointSource,
}

impl TonePoints {
    pub fn new(black_density: f64, white_density: f64, source: PointSource) -> TonePoints {
        TonePoints {
            black_density,
            white_density,
            source,
        }
    }
}

/// How many picture-area pixels have each raw value.
pub struct Histogram {
    counts: Vec<u32>,
    total: u64,
}

impl Histogram {
    /// Count the frame's pixels, leaving out `margin_mm` on each side (frame gap and film edge,
    /// which would otherwise pull the black point). The margin never takes more than a quarter
    /// of each side, so at least the central half of the frame is always counted.
    pub fn from_frame(frame: &GrayFrame, margin_mm: f64) -> Histogram {
        let margin = (margin_mm * frame.dpi / 25.4).round().max(0.0) as u32;
        let mx = margin.min(frame.width / 4);
        let my = margin.min(frame.height / 4);
        let mut counts = vec![0u32; 65536];
        let mut total = 0u64;
        for y in my..frame.height - my {
            for &v in &frame.row(y)[mx as usize..(frame.width - mx) as usize] {
                counts[usize::from(v)] += 1;
                total += 1;
            }
        }
        Histogram { counts, total }
    }

    /// The raw value at which, counting from the brightest raw value down, more than
    /// `fraction` of the counted pixels have been passed.
    fn at_bright_end(&self, fraction: f64) -> u16 {
        let limit = fraction * self.total as f64;
        let mut running = 0u64;
        // A running total over the histogram finds a percentile in one pass, without sorting.
        for v in (0..=u16::MAX).rev() {
            running += u64::from(self.counts[usize::from(v)]);
            if running as f64 > limit {
                return v;
            }
        }
        0
    }

    /// Like [`at_bright_end`](Self::at_bright_end), counting up from `floor` (exclusive)
    /// towards brighter values; values at or below `floor` are ignored entirely.
    fn at_dense_end(&self, fraction: f64, floor: u16) -> Option<u16> {
        let counted: u64 = self.counts[usize::from(floor) + 1..]
            .iter()
            .map(|&c| u64::from(c))
            .sum();
        let limit = fraction * counted as f64;
        let mut running = 0u64;
        for v in floor.checked_add(1)?..=u16::MAX {
            running += u64::from(self.counts[usize::from(v)]);
            if running as f64 > limit {
                return Some(v);
            }
        }
        None
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Black and white points measured on a frame, and whether the frame is probably blank.
///
/// Black is the density of the least dense picture area (film base and shadows), white that of
/// the densest (highlights), each ignoring a small fraction of pixels so a few specks cannot
/// decide them. Pixels denser than black by more than `opaque_range` are dust, not silver, and
/// are left out of the white point.
pub fn auto_points(hist: &Histogram, profile: &Profile) -> (TonePoints, bool) {
    let black = round3(density(hist.at_bright_end(profile.black_clip)));
    let opaque_floor = raw_at(black + profile.opaque_range).floor().max(0.0) as u16;
    let white = match hist.at_dense_end(profile.white_clip, opaque_floor) {
        Some(v) => round3(density(v)),
        None => black,
    };
    if white - black < profile.min_range {
        let points = TonePoints::new(
            black,
            round3(black + profile.nominal_range),
            PointSource::BlankDefault,
        );
        return (points, true);
    }
    (TonePoints::new(black, white, PointSource::Auto), false)
}

/// The density of the densest silver on the frame: the densest counted pixel that is not
/// opaque (not more than `opaque_range` denser than `black_density`). Dust removal treats only
/// pixels clearly denser than this as dust, so stars and other small bright details, which are
/// silver, are never mistaken for it (research P8).
pub fn densest_silver(hist: &Histogram, black_density: f64, profile: &Profile) -> f64 {
    let opaque_floor = raw_at(black_density + profile.opaque_range)
        .floor()
        .max(0.0) as u16;
    match hist.at_dense_end(0.0, opaque_floor) {
        Some(v) => density(v),
        None => black_density,
    }
}

/// The lookup table from raw gray to positive 16-bit gray (research P4): density mapped
/// linearly from black to white. Black and white are the only adjustments.
pub fn tone_table(points: &TonePoints) -> Vec<u16> {
    let black = points.black_density;
    let range = (points.white_density - black).max(1e-6);
    // Build the table with an iterator: one entry for every possible raw value, collected
    // into a `Vec`. Indexing it with a pixel's raw value gives that pixel's output.
    // Floating-point results can differ between machines in the last bit, but on one machine
    // the same operations on the same inputs always give the same result, so the same frame
    // and settings always give the same positive (spec FR-007).
    (0..=u16::MAX)
        .map(|raw| {
            let t = ((density(raw) - black) / range).clamp(0.0, 1.0);
            // `t` is in 0..=1, so the product fits a u16 after rounding.
            (t * FULL).round() as u16
        })
        .collect()
}

/// Replace every pixel with its entry in `table`.
pub fn apply(table: &[u16], frame: &mut GrayFrame) {
    for p in frame.pixels.iter_mut() {
        *p = table[usize::from(*p)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::film::FilmStock;

    fn raw(d: f64) -> u16 {
        raw_at(d).round() as u16
    }

    fn profile() -> Profile {
        FilmStock::Bw.profile()
    }

    /// A 200 × 100 frame at 254 dpi (10 px per mm) filled by `f(x, y)` in density.
    fn frame(f: impl Fn(u32, u32) -> f64) -> GrayFrame {
        let mut pixels = Vec::new();
        for y in 0..100 {
            for x in 0..200 {
                pixels.push(raw(f(x, y)));
            }
        }
        GrayFrame::from_pixels(200, 100, 254.0, pixels)
    }

    fn points(f: &GrayFrame) -> (TonePoints, bool) {
        auto_points(&Histogram::from_frame(f, profile().margin_mm), &profile())
    }

    #[test]
    fn density_round_trip() {
        assert_eq!(density(65535), 0.0);
        assert!((density(raw(1.5)) - 1.5).abs() < 0.001);
        assert!(density(0).is_finite());
    }

    #[test]
    fn uniform_frame_is_probably_blank() {
        let (p, blank) = points(&frame(|_, _| 0.3));
        assert!(blank);
        assert_eq!(p.source, PointSource::BlankDefault);
        assert!((p.black_density - 0.3).abs() < 0.002);
        assert!((p.white_density - 1.5).abs() < 0.002);
    }

    #[test]
    fn two_level_frame_gives_both_densities() {
        let (p, blank) = points(&frame(|x, _| if x < 100 { 0.25 } else { 1.4 }));
        assert!(!blank);
        assert_eq!(p.source, PointSource::Auto);
        assert!((p.black_density - 0.25).abs() < 0.002, "{p:?}");
        assert!((p.white_density - 1.4).abs() < 0.002, "{p:?}");
    }

    #[test]
    fn clear_border_is_ignored() {
        // 2 mm (20 px) of clear base around a 0.4–1.2 D picture.
        let bordered = frame(|x, y| {
            if !(20..180).contains(&x) || !(20..80).contains(&y) {
                0.05
            } else if x < 100 {
                0.4
            } else {
                1.2
            }
        });
        let (p, _) = points(&bordered);
        assert!((p.black_density - 0.4).abs() < 0.002, "{p:?}");
    }

    #[test]
    fn a_few_dense_pixels_do_not_move_the_white_point() {
        // 2 of 9600 counted pixels (0.02%) are much denser silver than the rest.
        let (p, _) = points(&frame(|x, y| match (x, y) {
            (50, 50) | (60, 50) => 1.9,
            _ if x < 100 => 0.3,
            _ => 1.2,
        }));
        assert!((p.white_density - 1.2).abs() < 0.002, "{p:?}");
    }

    #[test]
    fn opaque_dust_is_left_out_of_the_white_point() {
        // 1% of the picture is opaque dust (density 3.3, more than 2 D above black).
        let (p, _) = points(&frame(|x, y| {
            if x % 10 == 0 && y % 10 == 0 {
                3.3
            } else if x < 100 {
                0.3
            } else {
                1.2
            }
        }));
        assert!((p.white_density - 1.2).abs() < 0.002, "{p:?}");
    }

    #[test]
    fn table_reverses_and_anchors() {
        let p = TonePoints::new(0.2, 1.4, PointSource::Auto);
        let table = tone_table(&p);
        assert_eq!(table.len(), 65536);
        // Denser negative (lower raw value) gives a lighter positive.
        assert!(table.windows(2).all(|w| w[0] >= w[1]));
        assert_eq!(table[usize::from(raw(0.2))], 0);
        assert_eq!(table[usize::from(raw(1.4))], 65535);
        assert_eq!(table[usize::from(raw(0.1))], 0);
        assert_eq!(table[0], 65535);
        let mid = table[usize::from(raw(0.8))];
        assert!((i32::from(mid) - 32768).abs() < 60, "{mid}");
    }

    #[test]
    fn table_is_deterministic() {
        let p = TonePoints::new(0.2, 1.4, PointSource::Auto);
        assert_eq!(tone_table(&p), tone_table(&p));
    }
}
