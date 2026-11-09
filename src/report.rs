//! Everything the tool prints: human-readable text, `--json` documents and verbose steps.
//!
//! Results go to stdout; errors and verbose steps go to stderr, so scripts can pipe stdout.

use serde::Serialize;

use crate::backend::DeviceInfo;
use crate::capability::{Capability, CapabilitySummary, Constraint, Infrared, Unit, Value};
use crate::error::ScanError;
use crate::film::FilmStock;
use crate::preset::{Preset, Quality, frame_pixels};
use crate::scan::{FilmMoved, Outcome, RunKind, RunOptions, RunReport, SavedFile};

/// Version of the `--json` format of `detect` and `capabilities`. Increases only for
/// incompatible changes.
const SCHEMA_VERSION: u32 = 1;

/// Version of the scanning commands' `--json` format: 2 since feature 003 (positives).
const SCAN_SCHEMA_VERSION: u32 = 2;

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
/// document on stdout, in the failed command's `schema_version`.
pub fn print_error(err: &ScanError, json: bool, schema_version: u32) {
    eprintln!("{err}");
    eprintln!("Next step: {}", err.next_step());
    if json {
        print_json(&ErrorReport {
            schema_version,
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

/// `--json` document for `scan`, `roll`, `preview` and `advance`
/// (`specs/002-film-scan/contracts/scan-output.schema.json`).
#[derive(Serialize)]
struct ScanReport {
    schema_version: u32,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    roll: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preset: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolution_dpi: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    film: Option<FilmStock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tone_points: Option<LockedPoints>,
    files: Vec<SavedFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    film_moved: Option<FilmMoved>,
}

/// The shared points of a `--match-tones` roll.
#[derive(Serialize)]
struct LockedPoints {
    black_density: f64,
    white_density: f64,
    matched_from_frame: u32,
}

#[derive(Serialize)]
struct FilmsReport {
    schema_version: u32,
    outcome: &'static str,
    films: Vec<FilmInfo>,
}

#[derive(Serialize)]
struct FilmInfo {
    value: &'static str,
    name: &'static str,
    film_type: &'static str,
    default: bool,
}

#[derive(Serialize)]
struct PresetsReport {
    schema_version: u32,
    outcome: &'static str,
    presets: Vec<PresetInfo>,
}

#[derive(Serialize)]
struct PresetInfo {
    name: &'static str,
    resolution_dpi: f64,
    width: u32,
    height: u32,
    file_bytes: u64,
    seconds_per_frame: Option<f64>,
    default: bool,
}

/// Print the result of a scanning command: a summary (or JSON) on stdout and, for a failure,
/// the message and next step on stderr. Progress lines were already printed while scanning.
pub fn print_run(report: &RunReport, opts: &RunOptions, json: bool) {
    let (message, next_step) = outcome_text(report, opts);
    if let Outcome::Failed(_) = report.outcome {
        if let Some(message) = &message {
            eprintln!("{message}");
        }
        if let Some(next) = &next_step {
            eprintln!("Next step: {next}");
        }
    }

    if json {
        let outcome = match &report.outcome {
            Outcome::Completed(_) => "completed",
            Outcome::EndOfFilm(_) => "end-of-film",
            Outcome::Failed(err) => err.outcome(),
        };
        print_json(&ScanReport {
            schema_version: SCAN_SCHEMA_VERSION,
            outcome,
            message,
            next_step,
            roll: report.roll.clone(),
            folder: report.folder.as_ref().map(|f| f.display().to_string()),
            preset: report.quality.map(Quality::name),
            resolution_dpi: report.quality.map(Quality::dpi),
            film: report.film,
            tone_points: locked_points(report),
            files: report.files.clone(),
            film_moved: match (report.kind, &report.outcome) {
                (RunKind::Advance, Outcome::Completed(_)) => Some(FilmMoved::Advanced),
                (RunKind::Advance, _) => Some(FilmMoved::None),
                _ => None,
            },
        });
        return;
    }

    match report.kind {
        RunKind::Scan | RunKind::Preview => {
            for file in &report.files {
                println!("{}", file.image.display());
            }
        }
        RunKind::Roll => {
            println!("{}", roll_summary(report));
            if let Some(line) = blank_summary(report) {
                println!("{line}");
            }
        }
        RunKind::Advance => {}
    }
    match &report.outcome {
        Outcome::Completed(Some(text)) | Outcome::EndOfFilm(text) => println!("{text}"),
        Outcome::Completed(None) | Outcome::Failed(_) => {}
    }
}

/// A matched roll's locked points, for JSON.
fn locked_points(report: &RunReport) -> Option<LockedPoints> {
    let p = report.tone_points?;
    Some(LockedPoints {
        black_density: p.black_density,
        white_density: p.white_density,
        matched_from_frame: p.matched_from_frame?,
    })
}

/// "Probably blank: 001, 007." when any frame was probably blank.
fn blank_summary(report: &RunReport) -> Option<String> {
    let blank: Vec<String> = report
        .files
        .iter()
        .filter(|f| f.probably_blank)
        .filter_map(|f| f.frame)
        .map(|n| format!("{n:03}"))
        .collect();
    (!blank.is_empty()).then(|| format!("Probably blank: {}.", blank.join(", ")))
}

/// "Saved 2 frames (001–002) to <folder> in 2 min 27 s."
fn roll_summary(report: &RunReport) -> String {
    let folder = report
        .folder
        .as_ref()
        .map(|f| f.display().to_string())
        .unwrap_or_default();
    let frames: Vec<u32> = report.files.iter().filter_map(|f| f.frame).collect();
    // A slice pattern: `[first, .., last]` matches a list of two or more and names its ends.
    let range = match frames.as_slice() {
        [] => return format!("Saved no frames in {}.", duration_text(report.seconds)),
        [only] => format!("1 frame ({only:03})"),
        [first, .., last] => format!("{} frames ({first:03}–{last:03})", frames.len()),
    };
    format!(
        "Saved {range} to {folder} in {}.",
        duration_text(report.seconds)
    )
}

/// The outcome's message and next step, if any. For a roll that stopped early, the next step
/// is the command that resumes it.
fn outcome_text(report: &RunReport, opts: &RunOptions) -> (Option<String>, Option<String>) {
    let err = match &report.outcome {
        Outcome::Completed(text) => return (text.clone(), None),
        Outcome::EndOfFilm(text) => return (Some(text.clone()), None),
        Outcome::Failed(err) => err,
    };
    let last_saved = report.files.last().and_then(|f| f.frame);
    let message = match (err, last_saved) {
        (ScanError::Interrupted, Some(n)) => {
            format!("Interrupted after frame {n:03}; scanner released.")
        }
        _ => err.to_string(),
    };
    let resume = match (report.kind, report.next_frame) {
        (RunKind::Roll, Some(next)) => Some(resume_command(report, opts, next)),
        _ => None,
    };
    let next_step = match (err, resume) {
        (ScanError::Interrupted, Some(cmd)) => cmd,
        (ScanError::Disconnected { .. }, Some(cmd)) => {
            format!("Check power and cable, and where the film is; then run: {cmd}")
        }
        (ScanError::ScanFailed { .. }, Some(cmd)) => {
            format!("Check the film for a jam (use the eject button if needed), then run: {cmd}")
        }
        (ScanError::OutputError { .. }, Some(cmd)) => {
            format!("Check free space and permissions of the output folder, then run: {cmd}")
        }
        _ => err.next_step(),
    };
    (Some(message), Some(next_step))
}

/// `primefilm roll --roll R --start-frame N`, repeating the quality, processing options and
/// output folder the owner chose. A matched roll whose tones are locked continues with those
/// points (`--black`, `--white`) so the rest of the roll matches (research P7).
fn resume_command(report: &RunReport, opts: &RunOptions, next: u32) -> String {
    let mut cmd = format!(
        "primefilm roll --roll {} --start-frame {next}",
        report.roll.as_deref().unwrap_or_default()
    );
    if opts.quality_given {
        match opts.quality {
            Quality::Preset(p) => cmd.push_str(&format!(" --preset {p}")),
            Quality::Custom(dpi) => cmd.push_str(&format!(" --resolution {dpi}")),
        }
    }
    for flag in &opts.process_flags {
        cmd.push(' ');
        cmd.push_str(flag);
    }
    match report.tone_points {
        Some(p) => cmd.push_str(&format!(
            " --black {:.3} --white {:.3}",
            p.black_density, p.white_density
        )),
        None if opts.match_tones => cmd.push_str(" --match-tones"),
        None => {}
    }
    if opts.output_given {
        cmd.push_str(&format!(
            " --output {}",
            shell_quote(&opts.output.display().to_string())
        ));
    }
    cmd
}

/// Quote `text` for a shell if it contains anything but safe characters.
fn shell_quote(text: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+:=@,".contains(c);
    if !text.is_empty() && text.chars().all(safe) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// "47 s" or "2 min 27 s".
fn duration_text(seconds: f64) -> String {
    let total = seconds.round() as u64;
    if total < 60 {
        format!("{total} s")
    } else {
        format!("{} min {} s", total / 60, total % 60)
    }
}

/// Print the preset table for `primefilm presets`.
pub fn print_presets(json: bool) {
    let presets: Vec<PresetInfo> = Preset::ALL
        .iter()
        .map(|p| {
            let (width, height) = frame_pixels(p.dpi());
            PresetInfo {
                name: p.name(),
                resolution_dpi: p.dpi(),
                width,
                height,
                // The 16-bit gray master; the JPEG share copy adds roughly a tenth.
                file_bytes: u64::from(width) * u64::from(height) * 2,
                seconds_per_frame: p.seconds_per_frame(),
                default: *p == Preset::DEFAULT,
            }
        })
        .collect();
    if json {
        print_json(&PresetsReport {
            schema_version: SCAN_SCHEMA_VERSION,
            outcome: "completed",
            presets,
        });
        return;
    }
    println!(
        "{:<10} {:>9}  {:>13}  {:>9}  {:<18}",
        "Preset", "dpi", "pixels", "file", "time per frame"
    );
    for p in &presets {
        let time = match p.seconds_per_frame {
            Some(s) => format!("about {}", duration_text(s)),
            None => "not yet measured".to_string(),
        };
        let marker = if p.default { "  (default)" } else { "" };
        println!(
            "{:<10} {:>9}  {:>13}  {:>9}  {:<18}{marker}",
            p.name,
            p.resolution_dpi,
            format!("{} × {}", p.width, p.height),
            size_text(p.file_bytes),
            time
        );
    }
    println!();
    println!("Every full scan is saved as a 16-bit gray positive (size shown) plus a JPEG share");
    println!("copy; presets change only the resolution. Use --resolution DPI for any other value");
    println!("from 25 to 5000 (the scanner's sensor resolves no more than 5000 dpi across the");
    println!("frame).");
}

/// Print the film stocks for `primefilm films`.
pub fn print_films(json: bool) {
    let films: Vec<FilmInfo> = FilmStock::ALL
        .iter()
        .map(|s| FilmInfo {
            value: s.value(),
            name: s.name(),
            film_type: s.film_type().id(),
            default: *s == FilmStock::DEFAULT,
        })
        .collect();
    if json {
        print_json(&FilmsReport {
            schema_version: SCAN_SCHEMA_VERSION,
            outcome: "completed",
            films,
        });
        return;
    }
    println!("{:<6} {:<33} Type", "Film", "Name");
    for (film, stock) in films.iter().zip(FilmStock::ALL) {
        let marker = if film.default { "  (default)" } else { "" };
        println!(
            "{:<6} {:<33} {}{marker}",
            film.value,
            film.name,
            stock.film_type().name()
        );
    }
    println!("Color film is not supported yet.");
}

/// "143 MB" or "1.1 GB".
fn size_text(bytes: u64) -> String {
    let mb = bytes as f64 / 1e6;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1000.0)
    } else {
        format!("{mb:.0} MB")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn roll_report(outcome: Outcome) -> RunReport {
        RunReport {
            kind: RunKind::Roll,
            roll: Some("r".to_string()),
            folder: Some(PathBuf::from("/x/r")),
            quality: Some(Quality::Preset(Preset::Web)),
            film: Some(FilmStock::Hp5),
            files: Vec::new(),
            outcome,
            next_frame: Some(3),
            tone_points: None,
            seconds: 147.0,
        }
    }

    fn opts(quality_given: bool, output: &str, output_given: bool) -> RunOptions {
        RunOptions {
            output: PathBuf::from(output),
            output_given,
            device: None,
            quality: Quality::Preset(Preset::Web),
            quality_given,
            process: crate::process::ProcessSettings::from_profile(FilmStock::Hp5),
            process_flags: Vec::new(),
            match_tones: false,
            keep_capture: None,
        }
    }

    #[test]
    fn resume_command_repeats_chosen_options() {
        let report = roll_report(Outcome::Failed(ScanError::Interrupted));
        assert_eq!(
            resume_command(&report, &opts(false, "/d", false), 3),
            "primefilm roll --roll r --start-frame 3"
        );
        assert_eq!(
            resume_command(&report, &opts(true, "/my scans", true), 3),
            "primefilm roll --roll r --start-frame 3 --preset web --output '/my scans'"
        );
    }

    #[test]
    fn resume_command_repeats_processing_and_locked_tones() {
        use crate::tone::{PointSource, TonePoints};
        let mut report = roll_report(Outcome::Failed(ScanError::Interrupted));
        let mut o = opts(false, "/d", false);
        o.process_flags = vec!["--film".into(), "hp5".into(), "--dust".into(), "off".into()];
        o.match_tones = true;
        assert_eq!(
            resume_command(&report, &o, 3),
            "primefilm roll --roll r --start-frame 3 --film hp5 --dust off --match-tones"
        );
        report.tone_points = Some(TonePoints {
            matched_from_frame: Some(1),
            ..TonePoints::new(0.142, 1.387, PointSource::Matched)
        });
        assert_eq!(
            resume_command(&report, &o, 3),
            "primefilm roll --roll r --start-frame 3 --film hp5 --dust off --black 0.142 --white 1.387"
        );
    }

    #[test]
    fn jam_next_step_resumes() {
        let report = roll_report(Outcome::Failed(ScanError::ScanFailed {
            frame: Some(3),
            text: "jam".to_string(),
        }));
        let (message, next) = outcome_text(&report, &opts(false, "/d", false));
        assert!(message.unwrap().contains("frame 003"));
        assert!(
            next.unwrap()
                .ends_with("primefilm roll --roll r --start-frame 3")
        );
    }

    #[test]
    fn durations_and_sizes() {
        assert_eq!(duration_text(47.4), "47 s");
        assert_eq!(duration_text(147.0), "2 min 27 s");
        assert_eq!(size_text(142_500_000), "142 MB");
        assert_eq!(size_text(1_099_500_000), "1.1 GB");
        assert_eq!(
            roll_summary(&roll_report(Outcome::EndOfFilm(String::new()))),
            "Saved no frames in 2 min 27 s."
        );
    }

    #[test]
    fn scan_report_field_names() {
        let doc = serde_json::to_value(ScanReport {
            schema_version: 1,
            outcome: "completed",
            message: None,
            next_step: None,
            roll: Some("r".into()),
            folder: None,
            preset: Some("web"),
            resolution_dpi: Some(1800.0),
            film: None,
            tone_points: None,
            files: Vec::new(),
            film_moved: None,
        })
        .unwrap();
        let keys: Vec<&str> = doc
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "files",
                "outcome",
                "preset",
                "resolution_dpi",
                "roll",
                "schema_version"
            ]
        );
    }
}
