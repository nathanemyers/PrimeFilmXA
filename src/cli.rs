//! Command-line arguments, defined with clap's derive API (contracts/cli.md).

use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use crate::output::{MAX_FRAME, validate_roll_name};
use crate::preset::{MAX_DPI, MIN_DPI, Preset};

// `#[derive(...)]` asks the compiler (or a library) to write code for us. `Debug` makes the
// type printable with `{:?}`; `Parser` (from clap) generates the argument parser, `--help`
// text and usage errors from the struct's fields and their `///` doc comments.
/// Check, inspect and scan film with the Pacific Image PrimeFilm XA film scanner.
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

    /// Number of frames on the simulated film strip (for tests)
    #[arg(long, global = true, hide = true, default_value_t = crate::backend::fake::DEFAULT_FRAMES)]
    pub simulate_frames: u32,

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
    /// Scan the frame in the scanner once (the film does not move)
    Scan {
        /// Roll name, used for the folder and file names (default: today's date)
        #[arg(long, value_parser = validate_roll_name)]
        roll: Option<String>,
        /// Frame number to save as (default: one past the highest in the roll folder)
        #[arg(long, value_parser = frame_number)]
        frame: Option<u32>,
        #[command(flatten)]
        quality: QualityArgs,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Scan every frame of a strip or roll, advancing the film after each, until the end
    Roll {
        /// Roll name, used for the folder and file names (default: today's date)
        #[arg(long, value_parser = validate_roll_name)]
        roll: Option<String>,
        /// Number of the first frame (default: one past the highest in the roll folder)
        #[arg(long, value_parser = frame_number)]
        start_frame: Option<u32>,
        /// Stop after this many frames
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        frames: Option<u32>,
        #[command(flatten)]
        quality: QualityArgs,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Quick low-resolution preview of the frame in the scanner (the film does not move)
    Preview {
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Move the film forward one frame without saving anything
    Advance {
        /// SANE device name (default: the first scanner found)
        #[arg(long)]
        device: Option<String>,
    },
    /// List the quality presets
    Presets,
}

// `#[derive(Args)]` makes a reusable group of options; `#[command(flatten)]` above splices it
// into each command that needs it, so the options are defined once.
/// Where to save scans, and which scanner to use.
#[derive(Debug, Clone, Args)]
pub struct TargetArgs {
    /// Output folder (default: $HOME/Pictures/primefilm)
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// SANE device name (default: the first scanner found)
    #[arg(long)]
    pub device: Option<String>,
}

/// Scan quality: a named preset or an explicit resolution, not both.
// `multiple = false` makes clap reject `--preset` and `--resolution` together (exit 2).
#[derive(Debug, Clone, Default, Args)]
#[group(multiple = false)]
pub struct QualityArgs {
    /// Quality preset (default: standard; see `primefilm presets`)
    #[arg(long, value_enum)]
    pub preset: Option<Preset>,
    /// Resolution in dpi, 25 to 5000, instead of a preset
    #[arg(long, value_parser = resolution)]
    pub resolution: Option<f64>,
}

/// Parse `--frame` / `--start-frame`: 1 to 999.
fn frame_number(text: &str) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(n) if (1..=MAX_FRAME).contains(&n) => Ok(n),
        _ => Err(format!("frame numbers go from 1 to {MAX_FRAME}")),
    }
}

/// Parse `--resolution`: a number of dpi within the scanner's range.
fn resolution(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(dpi) if (MIN_DPI..=MAX_DPI).contains(&dpi) => Ok(dpi),
        _ => Err(format!("resolution must be {MIN_DPI} to {MAX_DPI} dpi")),
    }
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
    /// No film in the scanner: every frame shows the bare light source.
    NoFilm,
    /// The scanner reports an error on frame 3.
    Jam,
    /// Frame 4 onward shows the bare light source (the strip ran out).
    EmptyGate,
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
            other => panic!("expected capabilities, got {other:?}"),
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
        assert_eq!(cli.simulate_frames, 6);
    }

    #[test]
    fn parses_test_flags() {
        let cli = Cli::try_parse_from([
            "primefilm",
            "--simulate",
            "empty-gate",
            "--simulate-frames",
            "3",
            "detect",
        ])
        .unwrap();
        assert_eq!(cli.simulate, Some(Scenario::EmptyGate));
        assert_eq!(cli.simulate_frames, 3);
    }

    #[test]
    fn scan_options() {
        let cli = Cli::try_parse_from([
            "primefilm",
            "scan",
            "--roll",
            "r1",
            "--frame",
            "7",
            "--preset",
            "web",
        ])
        .unwrap();
        match cli.command {
            Command::Scan {
                roll,
                frame,
                quality,
                ..
            } => {
                assert_eq!(roll.as_deref(), Some("r1"));
                assert_eq!(frame, Some(7));
                assert_eq!(quality.preset, Some(Preset::Web));
            }
            other => panic!("expected scan, got {other:?}"),
        }
    }

    #[test]
    fn usage_errors() {
        for args in [
            vec!["primefilm", "scan", "--roll", "a/b"],
            vec!["primefilm", "scan", "--frame", "0"],
            vec!["primefilm", "scan", "--frame", "1000"],
            vec!["primefilm", "scan", "--preset", "huge"],
            vec!["primefilm", "scan", "--resolution", "30000"],
            vec![
                "primefilm",
                "scan",
                "--preset",
                "web",
                "--resolution",
                "2400",
            ],
            vec!["primefilm", "roll", "--frames", "0"],
        ] {
            assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn rejects_unknown_scenario() {
        assert!(Cli::try_parse_from(["primefilm", "--simulate", "nope", "detect"]).is_err());
    }
}
