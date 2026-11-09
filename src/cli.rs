//! Command-line arguments, defined with clap's derive API (contracts/cli.md).

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

// `#[derive(...)]` asks the compiler (or a library) to write code for us. `Debug` makes the
// type printable with `{:?}`; `Parser` (from clap) generates the argument parser, `--help`
// text and usage errors from the struct's fields and their `///` doc comments.
/// Check and inspect the Pacific Image PrimeFilm XA film scanner.
#[derive(Debug, Parser)]
#[command(name = "primefilm", version)]
pub struct Cli {
    /// Show detection steps on stderr; repeat (-vv) to add SANE driver debug output
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Print machine-readable JSON on stdout
    #[arg(long, global = true)]
    pub json: bool,

    /// Use a simulated scanner instead of SANE (for tests)
    #[arg(long, value_enum, global = true, hide = true)]
    pub simulate: Option<Scenario>,

    /// Load this SANE backend instead of pieusb (for tests)
    #[arg(long, global = true, hide = true)]
    pub sane_backend: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check whether the PrimeFilm XA is connected and usable
    Detect,
    /// List every setting the scanner exposes
    Capabilities {
        /// SANE device name to query (default: the first scanner found)
        #[arg(long)]
        device: Option<String>,
    },
}

/// What the simulated scanner does. clap spells these in kebab-case on the command line,
/// e.g. `--simulate no-scanner`.
// `Clone, Copy` let a `Scenario` be duplicated by simple assignment, like an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Scenario {
    Ready,
    NoScanner,
    Busy,
    PermissionDenied,
    DriverMissing,
    Disconnect,
    Interrupt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags_before_subcommand() {
        let cli = Cli::try_parse_from([
            "primefilm",
            "-vv",
            "--json",
            "capabilities",
            "--device",
            "X",
        ])
        .unwrap();
        assert_eq!(cli.verbose, 2);
        assert!(cli.json);
        match cli.command {
            Command::Capabilities { device } => assert_eq!(device.as_deref(), Some("X")),
            Command::Detect => panic!("expected capabilities"),
        }
    }

    #[test]
    fn parses_global_flags_after_subcommand() {
        let cli = Cli::try_parse_from([
            "primefilm",
            "detect",
            "--json",
            "-v",
            "--simulate",
            "no-scanner",
        ])
        .unwrap();
        assert!(cli.json);
        assert_eq!(cli.verbose, 1);
        assert_eq!(cli.simulate, Some(Scenario::NoScanner));
        assert!(matches!(cli.command, Command::Detect));
    }

    #[test]
    fn rejects_unknown_scenario() {
        assert!(Cli::try_parse_from(["primefilm", "--simulate", "nope", "detect"]).is_err());
    }
}
