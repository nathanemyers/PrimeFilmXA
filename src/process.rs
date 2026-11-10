//! The per-frame processing pipeline for B&W negatives (research P9, as revised in P16).
//!
//! 1. Measure black and white points on the picture area.
//! 2. Remove dust if asked (off by default), then measure again.
//! 3. Map every pixel through one tone table: negative densities to positive tones.
//!
//! Everything here works on a [`GrayFrame`] in memory and knows nothing about the scanner.

use std::time::Instant;

use crate::dust;
use crate::error::ScanError;
use crate::film::{DustConstants, DustLevel, FilmStock, Profile};
use crate::gray::GrayFrame;
use crate::tone::{self, Histogram, TonePoints};

/// What the owner asked for (data-model "Processing Settings").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessSettings {
    pub stock: FilmStock,
    pub dust: DustLevel,
}

impl ProcessSettings {
    /// The stock's defaults.
    pub fn from_profile(stock: FilmStock) -> ProcessSettings {
        ProcessSettings {
            stock,
            dust: stock.profile().dust,
        }
    }
}

/// What processing did to one frame (data-model "Frame Result").
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessedFrame {
    pub profile: Profile,
    pub points: TonePoints,
    pub probably_blank: bool,
    /// Dust constants used, in pixels; `None` when dust removal was off.
    pub dust: Option<DustConstants>,
    /// Number of specks and hairs repaired.
    pub dust_repaired: u32,
    /// Processing time only.
    pub seconds: f64,
}

/// Turn the raw gray negative in `frame` into the positive, in place.
pub fn process(
    frame: &mut GrayFrame,
    settings: &ProcessSettings,
) -> Result<ProcessedFrame, ScanError> {
    let started = Instant::now();
    if !frame.is_complete() {
        return Err(ScanError::ScanFailed {
            frame: None,
            text: "the frame was incomplete and could not be processed".to_string(),
        });
    }
    let profile = settings.stock.profile();
    let hist = Histogram::from_frame(frame, profile.margin_mm);
    let (mut points, probably_blank) = tone::auto_points(&hist, &profile);

    let dust = settings.dust.constants(frame.dpi);
    let mut dust_repaired = 0;
    if let Some(constants) = &dust {
        let silver = tone::densest_silver(&hist, points.black_density, &profile);
        dust_repaired = dust::remove(frame, silver, constants);
        // A large speck could have nudged the points; measure the clean frame.
        if dust_repaired > 0 {
            points =
                tone::auto_points(&Histogram::from_frame(frame, profile.margin_mm), &profile).0;
        }
    }

    tone::apply(&tone::tone_table(&points), frame);
    Ok(ProcessedFrame {
        profile,
        points,
        probably_blank,
        dust,
        dust_repaired,
        seconds: started.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tone::{PointSource, raw_at};

    fn raw(d: f64) -> u16 {
        raw_at(d).round() as u16
    }

    /// A 21-step density wedge, 0.10 to 1.90 D in 0.09 D steps, each band 20 px wide, inside a
    /// 20 px border of clear film base, at 254 dpi (10 px per mm, so the border is the 2 mm
    /// margin).
    fn wedge() -> GrayFrame {
        let (width, height) = (460u32, 100u32);
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let d = if !(20..440).contains(&x) || !(20..80).contains(&y) {
                    0.03
                } else {
                    0.10 + 0.09 * f64::from((x - 20) / 20)
                };
                pixels.push(raw(d));
            }
        }
        GrayFrame::from_pixels(width, height, 254.0, pixels)
    }

    fn band_means(frame: &GrayFrame) -> Vec<f64> {
        (0..21)
            .map(|band| {
                let mut sum = 0.0;
                let mut n = 0.0;
                for y in 30..70 {
                    for x in 22 + band * 20..38 + band * 20 {
                        sum += f64::from(frame.row(y)[x as usize]);
                        n += 1.0;
                    }
                }
                sum / n
            })
            .collect()
    }

    #[test]
    fn step_wedge_becomes_a_reversed_full_range_positive() {
        let mut frame = wedge();
        let settings = ProcessSettings::from_profile(FilmStock::Bw);
        let result = process(&mut frame, &settings).unwrap();
        assert!(!result.probably_blank);
        assert_eq!(result.points.source, PointSource::Auto);
        assert_eq!(result.dust, None, "dust removal is off by default");
        let means = band_means(&frame);
        for pair in means.windows(2) {
            // Denser bands are lighter in the positive, and every step is distinguishable.
            assert!(pair[1] - pair[0] > 500.0, "{means:?}");
        }
        assert!(means[0] < 0.03 * 65535.0, "{means:?}");
        assert!(means[20] > 0.97 * 65535.0, "{means:?}");
    }

    #[test]
    fn processing_is_deterministic() {
        let settings = ProcessSettings {
            dust: DustLevel::Normal,
            ..ProcessSettings::from_profile(FilmStock::Hp5)
        };
        let mut a = wedge();
        let mut b = wedge();
        let ra = process(&mut a, &settings).unwrap();
        let rb = process(&mut b, &settings).unwrap();
        assert_eq!(a.pixels, b.pixels);
        assert_eq!(ra.points, rb.points);
    }

    /// Processing time for a full `standard` frame (5173 × 3443) with dust removal, against
    /// the SC-002 budget of 18 s on top of a 91 s scan. Run on demand, in release mode:
    /// `cargo test --release processing_time -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn processing_time_at_standard() {
        let (width, height) = (5173u32, 3443u32);
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let mut d = 0.2 + 1.2 * f64::from(x) / f64::from(width);
                if x % 397 == 0 && y % 211 == 0 {
                    d = 3.3; // a speck every so often
                }
                pixels.push(raw(d));
            }
        }
        let mut frame = GrayFrame::from_pixels(width, height, 3600.0, pixels);
        let settings = ProcessSettings {
            dust: DustLevel::Normal,
            ..ProcessSettings::from_profile(FilmStock::Hp5)
        };
        let result = process(&mut frame, &settings).unwrap();
        println!(
            "standard frame: {:.2} s, {} specks repaired",
            result.seconds, result.dust_repaired
        );
        assert!(result.seconds < 18.0);
    }

    #[test]
    fn incomplete_frame_is_an_error() {
        let mut frame = GrayFrame::from_pixels(4, 4, 300.0, vec![0; 15]);
        let settings = ProcessSettings::from_profile(FilmStock::Bw);
        assert!(process(&mut frame, &settings).is_err());
    }
}
