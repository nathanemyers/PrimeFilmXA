//! `primefilm`: a command-line tool for the Pacific Image PrimeFilm XA film scanner.
//!
//! This file is the root of the library crate. A *crate* is one unit of compilation in Rust;
//! this project has two: the library (this file and the modules below it) and the binary
//! (`src/main.rs`), which is a thin wrapper that calls [`run`]. Splitting them lets unit tests
//! reach internal modules while integration tests in `tests/` run the real binary.

// `unsafe` is forbidden everywhere except `backend::sane`, which wraps the SANE C library
// (constitution Principle VI). `deny` makes any `unsafe` block elsewhere a compile error.
#![deny(unsafe_code)]

// `mod name;` tells the compiler to load `src/name.rs` (or `src/name/mod.rs`) as a module.
// `pub` makes the module visible outside this crate, so `main.rs` and the tests can use it.
// Items are private by default in Rust; you opt in to sharing them.
pub mod backend;
pub mod capability;
pub mod cli;
pub mod dust;
pub mod error;
pub mod film;
pub mod frame_stats;
pub mod gray;
pub mod jpeg;
pub mod output;
pub mod preset;
pub mod process;
pub mod report;
pub mod scan;
pub mod tiff;
pub mod tone;
pub mod usb;

use std::sync::atomic::{AtomicBool, Ordering};

use backend::fake::FakeBackend;
use backend::sane::SaneBackend;
use backend::{ScannerBackend, choose_device};
use capability::CapabilitySummary;
use cli::{Cli, Command, ProcessArgs, QualityArgs, TargetArgs};
use error::ScanError;
use film::DustLevel;
use preset::{Preset, Quality};
use report::Logger;
use scan::{Outcome, RunOptions, RunReport};

/// The SANE driver used when `--sane-backend` is not given.
pub const DEFAULT_SANE_BACKEND: &str = "pieusb";

// Ownership and borrowing: every value in Rust has one owner, and is freed when its owner goes
// out of scope. `&Cli` *borrows* the parsed arguments: we can read them but the caller keeps
// ownership. Likewise `String` is an owned, growable string while `&str` is a borrowed view of
// one, and `Vec<T>` is an owned list while `&[T]` is a borrowed view of one. Functions here
// take borrowed views when they only need to read, so callers don't have to give anything up.
// `Option<T>` is either `Some(value)` or `None`; Rust has no null, so "might be missing" is
// spelled out in the type and the compiler makes us handle both cases.
/// The SANE backend this run will load: `--sane-backend` if given, otherwise `pieusb`.
pub fn sane_backend_name(cli: &Cli) -> &str {
    cli.sane_backend.as_deref().unwrap_or(DEFAULT_SANE_BACKEND)
}

/// A run that ended in failure. The scanning commands print their own report, error included,
/// so `reported` tells `main.rs` whether it still needs to print the error.
#[derive(Debug)]
pub struct Failure {
    pub error: ScanError,
    pub reported: bool,
}

// Implementing the standard `From` trait teaches the `?` operator a conversion: in a function
// returning `Result<_, Failure>`, `something()?` on a `Result<_, ScanError>` now turns the
// error into a `Failure` automatically, so the detect and capabilities code is unchanged.
impl From<ScanError> for Failure {
    fn from(error: ScanError) -> Failure {
        Failure {
            error,
            reported: false,
        }
    }
}

/// Run the command chosen on the command line.
///
/// `cancel` is set to `true` by the Ctrl-C handler (see `main.rs`); long operations check it
/// and stop with [`ScanError::Interrupted`].
pub fn run(cli: &Cli, log: &Logger, cancel: &AtomicBool) -> Result<(), Failure> {
    match cli.simulate {
        Some(scenario) => dispatch(
            &mut FakeBackend::new(scenario, cli.simulate_frames),
            cli,
            log,
            cancel,
        ),
        None => {
            let mut backend = SaneBackend::new(sane_backend_name(cli), *log);
            dispatch(&mut backend, cli, log, cancel)
        }
    }
}

// Generics: `<B: ScannerBackend>` means "this function works for any type `B` that implements
// the `ScannerBackend` trait". The compiler makes a separate copy for `FakeBackend` and for
// `SaneBackend`, so the command logic below is written once and shared by the real scanner and
// the simulated one. That is what lets the tests exercise the same code path the user runs.
fn dispatch<B: ScannerBackend>(
    backend: &mut B,
    cli: &Cli,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<(), Failure> {
    match &cli.command {
        Command::Detect => detect(backend, cli.json, log, cancel)?,
        Command::Capabilities { device } => {
            capabilities(backend, device.as_deref(), cli.json, log, cancel)?
        }
        Command::Scan {
            roll,
            frame,
            quality,
            process,
            dust,
            target,
        } => {
            let opts = run_options(cli, target, quality, process, *dust, false)?;
            // `unwrap_or_else` runs the function only when no roll name was given.
            let roll = roll.clone().unwrap_or_else(output::default_roll_name);
            let report = scan::scan_single(backend, &roll, *frame, &opts, log, cancel)?;
            finish(&report, &opts, cli.json)?;
        }
        Command::Roll {
            roll,
            start_frame,
            frames,
            quality,
            process,
            dust,
            match_tones,
            target,
        } => {
            let opts = run_options(cli, target, quality, process, *dust, *match_tones)?;
            let roll = roll.clone().unwrap_or_else(output::default_roll_name);
            let report =
                scan::scan_roll(backend, &roll, *start_frame, *frames, &opts, log, cancel)?;
            finish(&report, &opts, cli.json)?;
        }
        Command::Preview { process, target } => {
            let opts = run_options(
                cli,
                target,
                &QualityArgs::default(),
                process,
                Some(DustLevel::Off),
                false,
            )?;
            let report = scan::preview(backend, &opts, log, cancel)?;
            finish(&report, &opts, cli.json)?;
        }
        Command::Advance { device } => {
            let target = TargetArgs {
                output: None,
                device: device.clone(),
            };
            // `advance` writes no files, so it must not fail just because $HOME is unset.
            let opts = RunOptions {
                output: std::path::PathBuf::new(),
                output_given: false,
                device: target.device,
                quality: Quality::Preset(Preset::DEFAULT),
                quality_given: false,
                process: ProcessArgs::default().settings(None),
                process_flags: Vec::new(),
                match_tones: false,
                keep_capture: None,
            };
            let report = scan::advance(backend, opts.device.as_deref(), log, cancel)?;
            finish(&report, &opts, cli.json)?;
        }
        Command::Presets => report::print_presets(cli.json),
        Command::Films => report::print_films(cli.json),
    }
    Ok(())
}

/// Turn the scanning commands' shared options into [`RunOptions`].
fn run_options(
    cli: &Cli,
    target: &TargetArgs,
    quality: &QualityArgs,
    process: &ProcessArgs,
    dust: Option<DustLevel>,
    match_tones: bool,
) -> Result<RunOptions, ScanError> {
    let output = match &target.output {
        Some(dir) => dir.clone(),
        None => output::default_output_root()?,
    };
    let chosen = match (quality.preset, quality.resolution) {
        (Some(preset), _) => Some(Quality::Preset(preset)),
        (None, Some(dpi)) => Some(Quality::Custom(dpi)),
        (None, None) => None,
    };
    Ok(RunOptions {
        output,
        output_given: target.output.is_some(),
        device: target.device.clone(),
        quality: chosen.unwrap_or(Quality::Preset(Preset::DEFAULT)),
        quality_given: chosen.is_some(),
        process: process.settings(dust),
        process_flags: process.flags(dust),
        match_tones,
        keep_capture: cli.keep_capture.clone(),
    })
}

/// Print a scanning command's report; a failed outcome becomes an already-reported failure.
fn finish(report: &RunReport, opts: &RunOptions, json: bool) -> Result<(), Failure> {
    report::print_run(report, opts, json);
    match &report.outcome {
        Outcome::Failed(error) => Err(Failure {
            error: error.clone(),
            reported: true,
        }),
        Outcome::Completed(_) | Outcome::EndOfFilm(_) => Ok(()),
    }
}

fn detect<B: ScannerBackend>(
    backend: &mut B,
    json: bool,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<(), ScanError> {
    let devices = backend.list_devices()?;
    check_cancel(cancel)?;
    log.step(&format!("{} device(s) listed", devices.len()));
    if devices.is_empty() {
        return Err(ScanError::NotFound);
    }
    report::print_devices(&devices, json);
    Ok(())
}

fn capabilities<B: ScannerBackend>(
    backend: &mut B,
    device: Option<&str>,
    json: bool,
    log: &Logger,
    cancel: &AtomicBool,
) -> Result<(), ScanError> {
    let devices = backend.list_devices()?;
    check_cancel(cancel)?;
    log.step(&format!("{} device(s) listed", devices.len()));
    let info = choose_device(&devices, device)?;

    let options = backend.read_capabilities(&info.name, cancel)?;
    let summary = CapabilitySummary::from_options(&options);
    report::print_capabilities(&info, &summary, &options, json);
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), ScanError> {
    if cancel.load(Ordering::SeqCst) {
        Err(ScanError::Interrupted)
    } else {
        Ok(())
    }
}
