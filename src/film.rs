//! Film types, film stocks and their processing profiles (research P6, data-model.md).
//!
//! Plain enums with `match`: adding a film type or stock later (color negative, another B&W
//! film) means adding a variant, and the compiler then points at every `match` that needs an
//! arm for it.

use clap::ValueEnum;
use serde::Serialize;

/// A family of films processed the same general way. Color negative comes in a later feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilmType {
    /// Silver-based black-and-white negative.
    BwNegative,
}

impl FilmType {
    /// The value recorded in settings records, e.g. `bw-negative`.
    pub fn id(self) -> &'static str {
        match self {
            FilmType::BwNegative => "bw-negative",
        }
    }

    /// The name shown by `primefilm films`.
    pub fn name(self) -> &'static str {
        match self {
            FilmType::BwNegative => "B&W negative",
        }
    }
}

/// The film the owner says is in the scanner (`--film`).
// `ValueEnum` lets clap accept the lowercase variant names (`bw`, `hp5`, `fp4`) on the command
// line; `Serialize` with `rename_all` writes the same names into JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FilmStock {
    /// Any other black-and-white negative
    Bw,
    /// Ilford HP5 Plus
    Hp5,
    /// Ilford FP4 Plus
    Fp4,
}

impl FilmStock {
    pub const DEFAULT: FilmStock = FilmStock::Bw;
    pub const ALL: [FilmStock; 3] = [FilmStock::Bw, FilmStock::Hp5, FilmStock::Fp4];

    /// The value used on the command line and in records, e.g. `hp5`.
    pub fn value(self) -> &'static str {
        match self {
            FilmStock::Bw => "bw",
            FilmStock::Hp5 => "hp5",
            FilmStock::Fp4 => "fp4",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FilmStock::Bw => "Generic black-and-white negative",
            FilmStock::Hp5 => "Ilford HP5 Plus",
            FilmStock::Fp4 => "Ilford FP4 Plus",
        }
    }

    pub fn film_type(self) -> FilmType {
        match self {
            FilmStock::Bw | FilmStock::Hp5 | FilmStock::Fp4 => FilmType::BwNegative,
        }
    }

    /// This stock's processing defaults. Every stock starts with the same values, but each has
    /// its own arm so one can be tuned without affecting the others (spec FR-012).
    pub fn profile(self) -> Profile {
        match self {
            FilmStock::Bw => Profile::BW_START,
            FilmStock::Hp5 => Profile::BW_START,
            FilmStock::Fp4 => Profile::BW_START,
        }
    }
}

/// Parse `--film`. `color` is recognised only to say it is not supported yet (spec FR-013).
pub fn parse_film(text: &str) -> Result<FilmStock, String> {
    if text.eq_ignore_ascii_case("color") || text.eq_ignore_ascii_case("colour") {
        return Err("Color film is not supported yet.".to_string());
    }
    // `from_str` is provided by `ValueEnum`; `true` ignores case.
    FilmStock::from_str(text, true).map_err(|_| {
        let values: Vec<&str> = FilmStock::ALL.iter().map(|s| s.value()).collect();
        format!("unknown film '{text}'; choose one of {}", values.join(", "))
    })
}

/// How hard dust removal works (`--dust`, research P8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DustLevel {
    Off,
    Gentle,
    Normal,
    Strong,
}

impl DustLevel {
    pub fn name(self) -> &'static str {
        match self {
            DustLevel::Off => "off",
            DustLevel::Gentle => "gentle",
            DustLevel::Normal => "normal",
            DustLevel::Strong => "strong",
        }
    }

    /// The level's constants in pixels for a frame scanned at `dpi`, or `None` for `off`.
    /// Sizes are physical (mm on the film), so dust removal behaves the same at every preset;
    /// each is at least one pixel so tiny test frames still work.
    pub fn constants(self, dpi: f64) -> Option<DustConstants> {
        // (margin in D, max area in mm², max span in mm, dilation in px), research P8.
        let (margin, area_mm2, span_mm, dilate_px) = match self {
            DustLevel::Off => return None,
            DustLevel::Gentle => (0.50, 0.05, 1.0, 1),
            DustLevel::Normal => (0.30, 0.10, 2.0, 2),
            DustLevel::Strong => (0.15, 0.25, 4.0, 3),
        };
        let px_per_mm = dpi / 25.4;
        Some(DustConstants {
            margin,
            max_area_px: (area_mm2 * px_per_mm * px_per_mm).ceil().max(1.0) as u32,
            max_span_px: (span_mm * px_per_mm).ceil().max(1.0) as u32,
            dilate_px,
        })
    }
}

/// Dust removal constants for one frame, in pixels (data-model "Dust Level").
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DustConstants {
    /// A pixel is a dust candidate when it is this much denser than the white point.
    pub margin: f64,
    /// Largest speck or hair removed, in pixels.
    pub max_area_px: u32,
    /// Longest side of a removed speck's bounding box, in pixels.
    pub max_span_px: u32,
    /// How far each speck is grown to cover its soft edge, in pixels.
    pub dilate_px: u32,
}

/// A stock's processing defaults (data-model "Processing Profile").
// A plain data struct. `Serialize` writes it into the settings record so every positive says
// exactly which constants made it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Profile {
    /// Fraction of picture-area pixels allowed to be darker than black.
    pub black_clip: f64,
    /// Fraction allowed to be lighter than white.
    pub white_clip: f64,
    /// Border ignored by the statistics on each side, in mm (frame gap, film edge).
    pub margin_mm: f64,
    /// Below this density range the frame is "probably blank".
    pub min_range: f64,
    /// Density range used instead for a probably blank frame.
    pub nominal_range: f64,
    /// Pixels this much denser than black are treated as opaque (dust), not silver, when
    /// setting the white point. A normally developed B&W negative's highlights stay well
    /// below it.
    pub opaque_range: f64,
    #[serde(skip)]
    pub brightness: i32,
    #[serde(skip)]
    pub contrast: i32,
    #[serde(skip)]
    pub dust: DustLevel,
}

impl Profile {
    /// Starting values for B&W negatives, to be tuned on real captures (research E11).
    const BW_START: Profile = Profile {
        black_clip: 0.001,
        white_clip: 0.0005,
        margin_mm: 2.0,
        min_range: 0.15,
        nominal_range: 1.2,
        opaque_range: 2.0,
        brightness: 0,
        contrast: 0,
        dust: DustLevel::Normal,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stock_is_bw_negative() {
        for stock in FilmStock::ALL {
            assert_eq!(stock.film_type(), FilmType::BwNegative);
            assert_eq!(parse_film(stock.value()), Ok(stock));
        }
        assert_eq!(FilmStock::DEFAULT, FilmStock::Bw);
        assert_eq!(parse_film("HP5"), Ok(FilmStock::Hp5));
    }

    #[test]
    fn color_is_not_supported_yet() {
        assert_eq!(
            parse_film("color"),
            Err("Color film is not supported yet.".to_string())
        );
        assert_eq!(
            parse_film("Color"),
            Err("Color film is not supported yet.".to_string())
        );
    }

    #[test]
    fn unknown_stock_lists_the_choices() {
        let err = parse_film("tri-x").unwrap_err();
        assert!(err.contains("bw, hp5, fp4"), "{err}");
    }

    #[test]
    fn profiles_start_identical() {
        let bw = FilmStock::Bw.profile();
        assert_eq!(FilmStock::Hp5.profile(), bw);
        assert_eq!(FilmStock::Fp4.profile(), bw);
        assert_eq!(bw.dust, DustLevel::Normal);
    }

    #[test]
    fn dust_constants_scale_with_resolution() {
        assert_eq!(DustLevel::Off.constants(3600.0), None);
        let normal = DustLevel::Normal.constants(3600.0).unwrap();
        // 0.10 mm² at 141.7 px/mm is 2009 px; 2 mm is 284 px.
        assert_eq!(normal.max_area_px, 2009);
        assert_eq!(normal.max_span_px, 284);
        assert_eq!(normal.dilate_px, 2);
        let tiny = DustLevel::Gentle.constants(30.0).unwrap();
        assert!(tiny.max_area_px >= 1 && tiny.max_span_px >= 1);
        let gentle = DustLevel::Gentle.constants(3600.0).unwrap();
        let strong = DustLevel::Strong.constants(3600.0).unwrap();
        assert!(gentle.margin > normal.margin && normal.margin > strong.margin);
        assert!(gentle.max_area_px < normal.max_area_px && normal.max_area_px < strong.max_area_px);
    }
}
