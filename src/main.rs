//! The `primefilm` binary: parse arguments, prepare SANE, handle Ctrl-C, run, set exit code.

// See `src/lib.rs`: no `unsafe` outside the SANE wrapper.
#![deny(unsafe_code)]

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use primefilm::backend::sane::config;
use primefilm::cli::Cli;
use primefilm::error::ScanError;
use primefilm::report::{self, Logger};

fn main() -> ExitCode {
    // On invalid arguments `parse` prints clap's usage message and exits with code 2.
    let cli = Cli::parse();
    let log = Logger::new(cli.verbose);
    match start(&cli, &log) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // By now the scanner and SANE have been released: they were dropped when `run`
            // returned.
            report::print_error(&err, cli.json);
            ExitCode::from(err.exit_code())
        }
    }
}

fn start(cli: &Cli, log: &Logger) -> Result<(), ScanError> {
    // Must happen before the Ctrl-C handler below starts its thread (see `config::prepare`).
    if cli.simulate.is_none() {
        let backend = primefilm::sane_backend_name(cli);
        let dir = config::prepare(backend, cli.verbose >= 2)?;
        log.step(&format!(
            "SANE config dir {} (backend: {backend})",
            dir.display()
        ));
    }

    // Sharing state with another thread: the `ctrlc` crate runs our closure on its own thread
    // when Ctrl-C is pressed. `Arc` ("atomically reference-counted") lets both threads own the
    // same flag, freeing it when the last owner is gone. `AtomicBool` can be read and written
    // from several threads at once without a lock or a data race. The option-reading loop
    // checks the flag and stops cleanly, so the normal `Drop` path releases the scanner.
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    // `move` makes the closure take ownership of `flag` (its own `Arc` handle).
    let installed = ctrlc::set_handler(move || {
        // A second Ctrl-C means a SANE call is stuck; exit outright. Linux releases the USB
        // claim when the process exits, so the scanner is still freed (research D5).
        if flag.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    });
    if let Err(err) = installed {
        log.step(&format!("could not install Ctrl-C handler: {err}"));
    }

    primefilm::run(cli, log, &cancel)
}
