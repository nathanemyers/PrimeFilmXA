//! Scanner options ("capabilities") and the summary of the ones that matter for film.
//!
//! Field names here are the JSON names in `contracts/output.schema.json`; see data-model.md.

use serde::{Deserialize, Serialize};

/// One setting the scanner exposes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capability {
    /// SANE option number, 1 or more (option 0 is the option count and is not listed).
    pub index: i32,
    /// Machine name, e.g. `resolution`. Empty for groups.
    pub name: String,
    /// Short human title.
    pub title: String,
    /// Longer help text. May be empty.
    pub description: String,
    pub value_type: ValueType,
    pub unit: Unit,
    /// Size of the value in bytes. For bool/int/fixed, `size / 4` is the number of values.
    pub size: i32,
    /// False when SANE marks the option inactive (it then has no readable value).
    pub active: bool,
    /// True when software can set it.
    pub settable: bool,
    /// True when the scanner can choose the value itself.
    pub automatic: bool,
    /// True for options SANE frontends hide by default.
    pub advanced: bool,
    pub constraint: Constraint,
    /// `None` (JSON `null`) for buttons, groups, inactive or unreadable options.
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Bool,
    Int,
    Fixed,
    String,
    Button,
    Group,
}

impl ValueType {
    /// Convert a `SANE_TYPE_*` number. `None` for numbers SANE does not define.
    pub fn from_sane(n: i32) -> Option<ValueType> {
        match n {
            0 => Some(ValueType::Bool),
            1 => Some(ValueType::Int),
            2 => Some(ValueType::Fixed),
            3 => Some(ValueType::String),
            4 => Some(ValueType::Button),
            5 => Some(ValueType::Group),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    None,
    Pixel,
    Bit,
    Mm,
    Dpi,
    Percent,
    Microsecond,
}

impl Unit {
    /// Convert a `SANE_UNIT_*` number. Unknown numbers become `Unit::None`.
    pub fn from_sane(n: i32) -> Unit {
        match n {
            1 => Unit::Pixel,
            2 => Unit::Bit,
            3 => Unit::Mm,
            4 => Unit::Dpi,
            5 => Unit::Percent,
            6 => Unit::Microsecond,
            _ => Unit::None,
        }
    }
}

/// Which values an option accepts.
// `tag = "kind"` makes serde write the variant name into a `"kind"` field, e.g.
// `{"kind": "range", "min": 0.0, "max": 36.0, "step": 0.0}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Constraint {
    #[serde(rename = "none")]
    None,
    /// `max` is inclusive; `step` 0 means any value.
    #[serde(rename = "range")]
    Range { min: f64, max: f64, step: f64 },
    #[serde(rename = "list")]
    NumberList { values: Vec<f64> },
    #[serde(rename = "strings")]
    StringList { values: Vec<String> },
}

/// An option's current value. Numbers are always lists, so array options are never truncated.
// `untagged` writes just the inner value (`[300]` or `"Color"`), with no variant name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(Vec<bool>),
    Int(Vec<i32>),
    Fixed(Vec<f64>),
    Text(String),
}

/// SANE's fixed-point numbers are 16.16: the real value is the integer divided by 2^16.
pub fn fixed_to_f64(raw: i32) -> f64 {
    raw as f64 / 65536.0
}

/// The key film-scanning facts (FR-004). `None` means "unknown".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySummary {
    pub resolutions_dpi: Option<Constraint>,
    pub color_modes: Option<Vec<String>>,
    pub bit_depths: Option<Vec<i64>>,
    pub scan_area_mm: Option<ScanArea>,
    pub infrared: Infrared,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScanArea {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Infrared {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "not available")]
    NotAvailable,
    #[serde(rename = "unknown")]
    Unknown,
}

impl CapabilitySummary {
    /// Derive the summary from the standard SANE option names (research D8).
    pub fn from_options(options: &[Capability]) -> CapabilitySummary {
        // Iterators and closures: `|name| ...` is a *closure*, a small unnamed function that
        // can use variables from around it (here, `options`). `options.iter()` yields each
        // option in turn and `.find(...)` stops at the first one the closure accepts. Chains
        // like this replace hand-written `for` loops with an index.
        let find = |name: &str| options.iter().find(|o| o.name == name);

        let range_max = |name: &str| match find(name).map(|o| &o.constraint) {
            Some(Constraint::Range { max, .. }) => Some(*max),
            _ => None,
        };

        let resolutions_dpi = find("resolution")
            .map(|o| o.constraint.clone())
            .filter(|c| *c != Constraint::None);

        let color_modes = match find("mode").map(|o| &o.constraint) {
            Some(Constraint::StringList { values }) => Some(values.clone()),
            _ => None,
        };

        let bit_depths = match find("depth").map(|o| &o.constraint) {
            Some(Constraint::NumberList { values }) => {
                Some(values.iter().map(|v| *v as i64).collect())
            }
            _ => None,
        };

        let scan_area_mm = match (range_max("br-x"), range_max("br-y")) {
            (Some(width), Some(height)) => Some(ScanArea { width, height }),
            _ => None,
        };

        let mentions_infrared = |s: &String| {
            let s = s.to_lowercase();
            s.contains("rgbi") || s.contains("infrared")
        };
        // `.iter().flatten()` over an `Option<Vec<_>>` visits nothing when it is `None`.
        let infrared = if color_modes.iter().flatten().any(mentions_infrared)
            || options.iter().any(|o| mentions_infrared(&o.name))
        {
            Infrared::Available
        } else if color_modes.is_some() {
            Infrared::NotAvailable
        } else {
            Infrared::Unknown
        };

        CapabilitySummary {
            resolutions_dpi,
            color_modes,
            bit_depths,
            scan_area_mm,
            infrared,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(name: &str, value_type: ValueType, constraint: Constraint) -> Capability {
        Capability {
            index: 1,
            name: name.to_string(),
            title: name.to_string(),
            description: String::new(),
            value_type,
            unit: Unit::None,
            size: 4,
            active: true,
            settable: true,
            automatic: false,
            advanced: false,
            constraint,
            value: None,
        }
    }

    fn strings(values: &[&str]) -> Constraint {
        Constraint::StringList {
            values: values.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn range(max: f64) -> Constraint {
        Constraint::Range {
            min: 0.0,
            max,
            step: 0.0,
        }
    }

    #[test]
    fn fixed_conversion() {
        assert_eq!(fixed_to_f64(65536), 1.0);
        assert_eq!(fixed_to_f64(-32768), -0.5);
    }

    #[test]
    fn summary_from_film_scanner_options() {
        let resolution = Constraint::NumberList {
            values: vec![1800.0, 3600.0, 7200.0],
        };
        let options = vec![
            option("resolution", ValueType::Int, resolution.clone()),
            option(
                "mode",
                ValueType::String,
                strings(&["Color", "Gray", "RGBI"]),
            ),
            option(
                "depth",
                ValueType::Int,
                Constraint::NumberList {
                    values: vec![8.0, 16.0],
                },
            ),
            option("br-x", ValueType::Fixed, range(36.0)),
            option("br-y", ValueType::Fixed, range(24.0)),
        ];
        let summary = CapabilitySummary::from_options(&options);
        assert_eq!(summary.resolutions_dpi, Some(resolution));
        assert_eq!(
            summary.color_modes,
            Some(vec!["Color".into(), "Gray".into(), "RGBI".into()])
        );
        assert_eq!(summary.bit_depths, Some(vec![8, 16]));
        assert_eq!(
            summary.scan_area_mm,
            Some(ScanArea {
                width: 36.0,
                height: 24.0
            })
        );
        assert_eq!(summary.infrared, Infrared::Available);
    }

    #[test]
    fn summary_of_nothing_is_unknown() {
        let summary = CapabilitySummary::from_options(&[]);
        assert_eq!(summary.resolutions_dpi, None);
        assert_eq!(summary.color_modes, None);
        assert_eq!(summary.bit_depths, None);
        assert_eq!(summary.scan_area_mm, None);
        assert_eq!(summary.infrared, Infrared::Unknown);
    }

    #[test]
    fn modes_without_infrared() {
        let options = vec![option(
            "mode",
            ValueType::String,
            strings(&["Color", "Gray"]),
        )];
        let summary = CapabilitySummary::from_options(&options);
        assert_eq!(summary.infrared, Infrared::NotAvailable);
    }

    #[test]
    fn infrared_option_name_counts() {
        let options = vec![
            option("mode", ValueType::String, strings(&["Color"])),
            option("infrared-clean", ValueType::Bool, Constraint::None),
        ];
        let summary = CapabilitySummary::from_options(&options);
        assert_eq!(summary.infrared, Infrared::Available);
    }

    #[test]
    fn value_json_shapes() {
        assert_eq!(
            serde_json::to_string(&Value::Int(vec![300])).unwrap(),
            "[300]"
        );
        assert_eq!(
            serde_json::to_string(&Value::Text("Color".into())).unwrap(),
            "\"Color\""
        );
        assert_eq!(
            serde_json::to_string(&Constraint::None).unwrap(),
            r#"{"kind":"none"}"#
        );
        assert_eq!(
            serde_json::to_string(&Infrared::NotAvailable).unwrap(),
            "\"not available\""
        );
    }
}
