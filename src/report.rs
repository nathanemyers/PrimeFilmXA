//! Everything the tool prints: human-readable text, `--json` documents and verbose steps.
//!
//! Results go to stdout; errors and verbose steps go to stderr, so scripts can pipe stdout.

use serde::Serialize;

use crate::backend::DeviceInfo;
use crate::capability::{Capability, CapabilitySummary, Constraint, Infrared, Unit, Value};
use crate::error::ScanError;

/// Version of the `--json` format. Increases only for incompatible changes.
const SCHEMA_VERSION: u32 = 1;

/// Prints `-v` progress steps on stderr.
#[derive(Debug, Clone, Copy)]
pub struct Logger {
    level: u8,
}

impl Logger {
    /// `level` is the number of `-v` flags.
    pub fn new(level: u8) -> Logger {
        Logger { level }
    }

    pub fn step(&self, msg: &str) {
        if self.level >= 1 {
            eprintln!("primefilm: {msg}");
        }
    }
}

// Serde: `#[derive(Serialize)]` generates the code that turns this struct into JSON, using the
// field names as keys. The JSON shape is therefore defined by these Rust types, and the
// compiler keeps the two in sync. The reports own copies of their data (`Vec`, `String`)
// rather than borrowing it, which costs a little copying but keeps the types simple.
#[derive(Serialize)]
struct ErrorReport {
    schema_version: u32,
    outcome: &'static str,
    message: String,
    next_step: String,
}

#[derive(Serialize)]
struct DetectReport {
    schema_version: u32,
    outcome: &'static str,
    devices: Vec<DeviceInfo>,
}

#[derive(Serialize)]
struct CapabilitiesReport {
    schema_version: u32,
    outcome: &'static str,
    device: DeviceInfo,
    summary: CapabilitySummary,
    options: Vec<Capability>,
}

/// Print a failure: always the message and next step on stderr; with `json`, also a JSON
/// document on stdout.
pub fn print_error(err: &ScanError, json: bool) {
    eprintln!("{err}");
    eprintln!("Next step: {}", err.next_step());
    if json {
        print_json(&ErrorReport {
            schema_version: SCHEMA_VERSION,
            outcome: err.outcome(),
            message: err.to_string(),
            next_step: err.next_step(),
        });
    }
}

/// Print the result of `detect`.
pub fn print_devices(devices: &[DeviceInfo], json: bool) {
    if json {
        print_json(&DetectReport {
            schema_version: SCHEMA_VERSION,
            outcome: "found",
            devices: devices.to_vec(),
        });
        return;
    }
    let noun = if devices.len() == 1 {
        "scanner"
    } else {
        "scanners"
    };
    println!("Found {} {noun}:", devices.len());
    for d in devices {
        println!("  {}  {} {} ({})", d.name, d.vendor, d.model, d.kind);
    }
}

/// Print the result of `capabilities`: a summary of the key facts, then every option.
pub fn print_capabilities(
    device: &DeviceInfo,
    summary: &CapabilitySummary,
    options: &[Capability],
    json: bool,
) {
    if json {
        print_json(&CapabilitiesReport {
            schema_version: SCHEMA_VERSION,
            outcome: "found",
            device: device.clone(),
            summary: summary.clone(),
            options: options.to_vec(),
        });
        return;
    }

    println!("{} {}  ({})", device.vendor, device.model, device.name);
    println!();
    println!("Summary");
    let resolutions = match &summary.resolutions_dpi {
        Some(c) => format!("{} dpi", constraint_text(c)),
        None => "unknown".to_string(),
    };
    let modes = match &summary.color_modes {
        Some(modes) => modes.join(", "),
        None => "unknown".to_string(),
    };
    let depths = match &summary.bit_depths {
        Some(depths) => join(depths),
        None => "unknown".to_string(),
    };
    let area = match &summary.scan_area_mm {
        Some(a) => format!("{:.1} × {:.1} mm", a.width, a.height),
        None => "unknown".to_string(),
    };
    let infrared = match summary.infrared {
        Infrared::Available => "available",
        Infrared::NotAvailable => "not available",
        Infrared::Unknown => "unknown",
    };
    println!("  Resolutions   {resolutions}");
    println!("  Color modes   {modes}");
    println!("  Bit depths    {depths}");
    println!("  Scan area     {area}");
    println!("  Infrared      {infrared}");

    for opt in options {
        if opt.value_type == crate::capability::ValueType::Group {
            println!();
            println!("{}", opt.title);
            continue;
        }
        let choices = match &opt.constraint {
            Constraint::None => String::new(),
            c => format!("[{}]", constraint_text(c)),
        };
        println!(
            "  {:<24} {:<16} {:<28} {}",
            opt.name,
            value_text(opt),
            choices,
            opt.title
        );
    }
}

/// The value column: `(inactive)` for inactive options, otherwise the value with its unit.
fn value_text(opt: &Capability) -> String {
    if !opt.active {
        return "(inactive)".to_string();
    }
    let text = match &opt.value {
        None => return "-".to_string(),
        Some(Value::Text(s)) => return s.clone(),
        Some(Value::Bool(v)) => numbers_text(v, |b| if *b { "yes" } else { "no" }.to_string()),
        Some(Value::Int(v)) => numbers_text(v, |n| n.to_string()),
        Some(Value::Fixed(v)) => numbers_text(v, |n| n.to_string()),
    };
    match unit_text(opt.unit) {
        "" => text,
        unit => format!("{text} {unit}"),
    }
}

/// Single values print bare, short arrays in brackets, long arrays (gamma tables) as a count;
/// `--json` always has every element.
fn numbers_text<T>(values: &[T], show: impl Fn(&T) -> String) -> String {
    match values {
        [one] => show(one),
        _ if values.len() > 8 => format!("[{} values]", values.len()),
        _ => format!(
            "[{}]",
            values.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn constraint_text(c: &Constraint) -> String {
    match c {
        Constraint::None => String::new(),
        Constraint::Range { min, max, step } if *step == 0.0 => format!("{min}..{max}"),
        Constraint::Range { min, max, step } => format!("{min}..{max} step {step}"),
        Constraint::NumberList { values } => join(values),
        Constraint::StringList { values } => values.join(", "),
    }
}

fn unit_text(unit: Unit) -> &'static str {
    match unit {
        Unit::None => "",
        Unit::Pixel => "px",
        Unit::Bit => "bit",
        Unit::Mm => "mm",
        Unit::Dpi => "dpi",
        Unit::Percent => "%",
        Unit::Microsecond => "µs",
    }
}

fn join<T: ToString>(values: &[T]) -> String {
    values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Print one JSON document on stdout.
fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        // Our report types contain only strings, numbers, booleans and lists, which always
        // serialize; this arm is here so a future mistake is reported instead of panicking.
        Err(err) => eprintln!("primefilm: internal error writing JSON: {err}"),
    }
}
