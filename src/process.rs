//! The per-frame processing pipeline for B&W negatives (research P9).
//!
//! 1. Measure black and white points on the picture area (unless given or locked).
//! 2. Remove dust, then measure again if the points are automatic.
//! 3. Map every pixel through one tone table: negative densities to positive tones.
//!
//! Everything here works on a [`GrayFrame`] in memory and knows nothing about the scanner.

use std::time::Instant;

use crate::dust;
use crate::error::ScanError;
use crate::film::{DustConstants, DustLevel, FilmStock, Profile};
use crate::gray::GrayFrame;
use crate::tone::{self, Histogram, PointSource, TonePoints};

/// What the owner asked for (data-model "Processing Settings"), already validated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessSettings {
    pub stock: FilmStock,
    /// −50…+50.
    pub brightness: i32,
    /// −50…+50.
    pub contrast: i32,
    /// Explicit (black, white) densities from `--black` and `--white`.
    pub points: Option<(f64, f64)>,
    pub dust: DustLevel,
}

impl ProcessSettings {
    /// The stock's defaults, with automatic points.
    pub fn from_profile(stock: FilmStock) -> ProcessSettings {
        let profile = stock.profile();
        ProcessSettings {
            stock,
            brightness: profile.brightness,
            contrast: profile.contrast,
            points: None,
            dust: profile.dust,
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
///
/// `locked` are the points of a `--match-tones` roll, if already locked; explicit points from
/// the settings win over them.
pub fn process(
    frame: &mut GrayFrame,
    settings: &ProcessSettings,
    locked: Option<&TonePoints>,
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
    // The automatic measurement runs even when points are given, to flag blank frames.
    let (auto, probably_blank) = tone::auto_points(&hist, &profile);
    let mut points = match (settings.points, locked) {
        (Some((black, white)), _) => TonePoints::new(black, white, PointSource::Explicit),
        (None, Some(locked)) => *locked,
        (None, None) => auto,
    };

    let dust = settings.dust.constants(frame.dpi);
    let mut dust_repaired = 0;
    if let Some(constants) = &dust {
        let silver = tone::densest_silver(&hist, points.black_density, &profile);
        dust_repaired = dust::remove(frame, silver, constants);
        // A large speck could have nudged the automatic points; measure the clean frame.
        if dust_repaired > 0 && points.source == PointSource::Auto {
            points = measure(frame, &profile).0;
        }
    }

    let table = tone::tone_table(&points, settings.brightness, settings.contrast);
    tone::apply(&table, frame);
    Ok(ProcessedFrame {
        profile,
        points,
        probably_blank,
        dust,
        dust_repaired,
        seconds: started.elapsed().as_secs_f64(),
    })
}

/// Shared tones for a `roll --match-tones` run (research P7, data-model "Tone Points").
///
/// Unlocked until the first frame that is not probably blank and has automatic points; that
/// frame's points are then used, marked `matched`, for it and every later frame.
#[derive(Debug, Clone, Default)]
pub struct ToneLock {
    locked: Option<TonePoints>,
}

impl ToneLock {
    pub fn new() -> ToneLock {
        ToneLock::default()
    }

    /// The locked points, once a frame has set them.
    pub fn points(&self) -> Option<&TonePoints> {
        self.locked.as_ref()
    }

    /// Look at a processed frame; lock its points if it is the first good one. Returns true
    /// when this frame locked them.
    pub fn observe(&mut self, frame: u32, result: &ProcessedFrame) -> bool {
        if self.locked.is_some()
            || result.probably_blank
            || result.points.source != PointSource::Auto
        {
            return false;
        }
        self.locked = Some(TonePoints {
            source: PointSource::Matched,
            matched_from_frame: Some(frame),
            ..result.points
        });
        true
    }
}

fn measure(frame: &GrayFrame, profile: &Profile) -> (TonePoints, bool) {
    let hist = Histogram::from_frame(frame, profile.margin_mm);
    tone::auto_points(&hist, profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tone::raw_at;

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

    fn settings() -> ProcessSettings {
        ProcessSettings {
            dust: DustLevel::Off,
            ..ProcessSettings::from_profile(FilmStock::Bw)
        }
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
        let result = process(&mut frame, &settings(), None).unwrap();
        assert!(!result.probably_blank);
        assert_eq!(result.points.source, PointSource::Auto);
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
        let mut a = wedge();
        let mut b = wedge();
        let ra = process(&mut a, &settings(), None).unwrap();
        let rb = process(&mut b, &settings(), None).unwrap();
        assert_eq!(a.pixels, b.pixels);
        assert_eq!(ra.points, rb.points);
        assert_eq!(ra.probably_blank, rb.probably_blank);
    }

    #[test]
    fn explicit_and_locked_points() {
        let mut frame = wedge();
        let explicit = ProcessSettings {
            points: Some((0.3, 1.2)),
            ..settings()
        };
        let locked = TonePoints::new(0.5, 1.0, PointSource::Matched);
        let result = process(&mut frame, &explicit, Some(&locked)).unwrap();
        assert_eq!(result.points.source, PointSource::Explicit);
        assert_eq!(result.points.black_density, 0.3);

        let mut frame = wedge();
        let result = process(&mut frame, &settings(), Some(&locked)).unwrap();
        assert_eq!(result.points, locked);
    }

    fn result(points: TonePoints, probably_blank: bool) -> ProcessedFrame {
        ProcessedFrame {
            profile: FilmStock::Bw.profile(),
            points,
            probably_blank,
            dust: None,
            dust_repaired: 0,
            seconds: 0.0,
        }
    }

    #[test]
    fn tone_lock_follows_the_data_model() {
        let mut lock = ToneLock::new();
        // A probably blank frame does not lock.
        let blank = TonePoints::new(0.1, 1.3, PointSource::BlankDefault);
        assert!(!lock.observe(1, &result(blank, true)));
        assert!(lock.points().is_none());
        // Explicit points never lock.
        let explicit = TonePoints::new(0.2, 1.0, PointSource::Explicit);
        assert!(!lock.observe(2, &result(explicit, false)));
        // The first good frame locks its points, marked as matched from that frame.
        let auto = TonePoints::new(0.15, 1.4, PointSource::Auto);
        assert!(lock.observe(3, &result(auto, false)));
        let locked = *lock.points().unwrap();
        assert_eq!(locked.source, PointSource::Matched);
        assert_eq!(locked.matched_from_frame, Some(3));
        assert_eq!((locked.black_density, locked.white_density), (0.15, 1.4));
        // Later frames do not change it.
        let other = TonePoints::new(0.3, 1.1, PointSource::Auto);
        assert!(!lock.observe(4, &result(other, false)));
        assert_eq!(lock.points(), Some(&locked));

        // A locked frame is processed with the locked points.
        let mut frame = wedge();
        let processed = process(&mut frame, &settings(), lock.points()).unwrap();
        assert_eq!(processed.points, locked);
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
        let settings = ProcessSettings::from_profile(FilmStock::Hp5);
        let result = process(&mut frame, &settings, None).unwrap();
        println!(
            "standard frame: {:.2} s, {} specks repaired",
            result.seconds, result.dust_repaired
        );
        assert!(result.seconds < 18.0);
    }

    #[test]
    fn incomplete_frame_is_an_error() {
        let mut frame = GrayFrame::from_pixels(4, 4, 300.0, vec![0; 15]);
        assert!(process(&mut frame, &settings(), None).is_err());
    }
}
