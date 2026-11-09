//! Named full-scan quality levels (data-model.md, "Preset").
//!
//! Presets change only the resolution: every full scan is RGBI at 16 bits whatever the preset
//! (constitution Principle I, spec clarifications).

use std::fmt;

use clap::ValueEnum;
use serde::Serialize;

/// The scanner's full scan area in mm (width, height), from the feature 001 recording of the
/// PrimeFilm XA's `br-x` / `br-y` ranges.
pub const SCAN_AREA_MM: (f64, f64) = (36.499786376953125, 24.297637939453125);

/// Lowest and highest useful resolution, in dpi. The scanner accepts up to 10000, but its
/// sensor is 7185 pixels wide, which is 5000 dpi across the frame: above 5000 dpi it only
/// steps the film more finely lengthwise, giving stretched images with non-square pixels
/// (hardware-validation.md, E8). So 5000 dpi is the true maximum.
pub const MIN_DPI: f64 = 25.0;
pub const MAX_DPI: f64 = 5000.0;

/// A named resolution. `ValueEnum` lets clap parse `--preset web` and list the valid names
/// in its error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Web,
    Standard,
    High,
}

// An `impl` block can hold associated constants as well as functions. `Preset::DEFAULT` and
// `Preset::ALL` read like fields of the type itself rather than of one value.
impl Preset {
    /// Used when the owner names no preset: mid-resolution, not the full 10000 dpi.
    pub const DEFAULT: Preset = Preset::Standard;

    /// Every preset, in the order `primefilm presets` lists them.
    pub const ALL: [Preset; 3] = [Preset::Web, Preset::Standard, Preset::High];

    pub fn dpi(self) -> f64 {
        match self {
            Preset::Web => 1800.0,
            Preset::Standard => 3600.0,
            Preset::High => 5000.0,
        }
    }

    /// The lower-case name used on the command line, in JSON and in the sidecar.
    pub fn name(self) -> &'static str {
        match self {
            Preset::Web => "web",
            Preset::Standard => "standard",
            Preset::High => "high",
        }
    }

    /// Measured time to scan one RGBI 16-bit frame at this preset, in seconds, on the
    /// PrimeFilm XA (hardware-validation.md, E8, 2026-11-09). `Option` leaves room for a
    /// preset that has not been measured.
    pub fn seconds_per_frame(self) -> Option<f64> {
        match self {
            Preset::Web => Some(49.0),
            Preset::Standard => Some(91.0),
            Preset::High => Some(314.0),
        }
    }
}

// Implementing the standard `Display` trait lets a preset be used in `format!("{preset}")`
// and `.to_string()`, like the built-in types.
impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The resolution chosen for a full scan: a named preset, or `--resolution` (FR-006).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quality {
    Preset(Preset),
    Custom(f64),
}

impl Quality {
    pub fn dpi(self) -> f64 {
        match self {
            Quality::Preset(preset) => preset.dpi(),
            Quality::Custom(dpi) => dpi,
        }
    }

    /// The preset name, or `custom`, as recorded in JSON and the settings record.
    pub fn name(self) -> &'static str {
        match self {
            Quality::Preset(preset) => preset.name(),
            Quality::Custom(_) => "custom",
        }
    }
}

/// Pixel size (width, height) of a full-frame scan at `dpi`: `floor(mm / 25.4 × dpi)`.
pub fn frame_pixels(dpi: f64) -> (u32, u32) {
    // `as u32` converts a float to an integer by dropping the fraction (and saturating at the
    // type's limits), which is exactly the `floor` we want for positive numbers.
    let width = (SCAN_AREA_MM.0 / 25.4 * dpi) as u32;
    let height = (SCAN_AREA_MM.1 / 25.4 * dpi) as u32;
    (width, height)
}

/// Size in bytes of an uncompressed RGBI 16-bit frame at `dpi`: 4 channels × 2 bytes per
/// pixel. The TIFF header and tags add well under a kilobyte.
pub fn rgbi16_bytes(dpi: f64) -> u64 {
    let (width, height) = frame_pixels(dpi);
    u64::from(width) * u64::from(height) * 8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table in data-model.md, exactly.
    #[test]
    fn pixel_sizes_match_data_model() {
        assert_eq!(frame_pixels(Preset::Web.dpi()), (2586, 1721));
        assert_eq!(frame_pixels(Preset::Standard.dpi()), (5173, 3443));
        assert_eq!(frame_pixels(Preset::High.dpi()), (7184, 4782));
    }

    #[test]
    fn file_sizes() {
        assert_eq!(rgbi16_bytes(Preset::Standard.dpi()), 5173 * 3443 * 8);
        // About 275 MB at high, the most the scanner can resolve.
        assert_eq!(rgbi16_bytes(Preset::High.dpi()), 7184 * 4782 * 8);
    }

    #[test]
    fn default_is_standard_and_listed() {
        assert_eq!(Preset::DEFAULT, Preset::Standard);
        assert!(Preset::ALL.contains(&Preset::DEFAULT));
        assert_eq!(Preset::High.to_string(), "high");
    }
}
