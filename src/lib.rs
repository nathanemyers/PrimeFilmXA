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
pub mod error;
pub mod report;
pub mod usb;

use std::sync::atomic::{AtomicBool, Ordering};

use backend::fake::FakeBackend;
use backend::sane::SaneBackend;
use backend::{DeviceInfo, ScannerBackend};
use capability::CapabilitySummary;
use cli::{Cli, Command};
use error::ScanError;
use report::Logger;

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

/// Run the command chosen on the command line.
///
/// `cancel` is set to `true` by the Ctrl-C handler (see `main.rs`); long operations check it
/// and stop with [`ScanError::Interrupted`].
pub fn run(cli: &Cli, log: &Logger, cancel: &AtomicBool) -> Result<(), ScanError> {
    match cli.simulate {
        Some(scenario) => dispatch(&mut FakeBackend::new(scenario), cli, log, cancel),
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
) -> Result<(), ScanError> {
    match &cli.command {
        Command::Detect => detect(backend, cli.json, log, cancel),
        Command::Capabilities { device } => {
            capabilities(backend, device.as_deref(), cli.json, log, cancel)
        }
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

/// Pick the `--device` scanner if given, otherwise the first one found.
fn choose_device(devices: &[DeviceInfo], wanted: Option<&str>) -> Result<DeviceInfo, ScanError> {
    // `.iter().find(|d| ...)` walks the list with a closure; see `capability.rs` for how
    // iterators and closures work. `.cloned()` turns the borrowed `&DeviceInfo` it finds into
    // an owned copy we can return, and `.ok_or(..)` turns `None` into an error.
    match wanted {
        Some(name) => devices
            .iter()
            .find(|d| d.name == name)
            .cloned()
            .ok_or(ScanError::NotFound),
        None => {
            let first = devices.first().cloned().ok_or(ScanError::NotFound)?;
            if devices.len() > 1 {
                eprintln!("using first scanner: {}", first.name);
            }
            Ok(first)
        }
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), ScanError> {
    if cancel.load(Ordering::SeqCst) {
        Err(ScanError::Interrupted)
    } else {
        Ok(())
    }
}
