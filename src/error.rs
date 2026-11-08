//! Every way a run can fail, with the message, next step and exit code for each.
//!
//! The wording comes from the outcomes table in `specs/001-scanner-detect/contracts/cli.md`.

// An `enum` is a type whose value is exactly one of several *variants*. Unlike C enums, each
// variant can carry its own data (`Busy` carries a device name, `Sane` a status code and text).
//
// Functions that can fail return `Result<T, ScanError>`: either `Ok(value)` or `Err(error)`.
// Callers write `something()?` to mean "if this is an `Err`, return it from my function now;
// otherwise give me the `Ok` value". That `?` operator is how errors travel up to `main.rs`,
// which prints them and picks the exit code.
//
// `thiserror::Error` derives the standard `Error` trait and `Display` (the human message) from
// the `#[error("...")]` attributes, so we don't write that boilerplate by hand. `{path}` inside
// a message refers to the variant's field of that name.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ScanError {
    #[error("No PrimeFilm XA found on USB.")]
    NotFound,

    #[error("The scanner is in use by another program.")]
    Busy { device: Option<String> },

    #[error("No permission to access {path}.")]
    PermissionDenied { path: String },

    #[error("The SANE pieusb driver is unavailable: {reason}.")]
    DriverUnavailable { reason: String },

    #[error("The scanner disconnected while {during}.")]
    Disconnected { during: String },

    #[error("Interrupted; scanner released.")]
    Interrupted,

    #[error("SANE error: {text}")]
    Sane { status: i32, text: String },
}

impl ScanError {
    /// The process exit code for this outcome (contracts/cli.md). Exit 2 is reserved for
    /// clap's usage errors and 0 for success.
    pub fn exit_code(&self) -> u8 {
        // `match` must handle every variant: if a new one is added to `ScanError`, this stops
        // compiling until it gets an exit code. `{ .. }` ignores the variant's fields.
        match self {
            ScanError::Sane { .. } => 1,
            ScanError::NotFound => 3,
            ScanError::Busy { .. } => 4,
            ScanError::PermissionDenied { .. } => 5,
            ScanError::DriverUnavailable { .. } => 6,
            ScanError::Disconnected { .. } => 7,
            ScanError::Interrupted => 130,
        }
    }

    /// The `outcome` string used in `--json` output.
    pub fn outcome(&self) -> &'static str {
        match self {
            ScanError::Sane { .. } => "error",
            ScanError::NotFound => "not-found",
            ScanError::Busy { .. } => "busy",
            ScanError::PermissionDenied { .. } => "permission-denied",
            ScanError::DriverUnavailable { .. } => "driver-unavailable",
            ScanError::Disconnected { .. } => "disconnected",
            ScanError::Interrupted => "interrupted",
        }
    }

    /// What the user should do about it.
    pub fn next_step(&self) -> String {
        let text = match self {
            ScanError::Sane { .. } => "Re-run with -vv and check the output.",
            ScanError::NotFound => {
                "Check the scanner is powered on and the cable is connected; \
                 try a port directly on the computer."
            }
            ScanError::Busy { .. } => "Close VueScan or other scanning programs and try again.",
            ScanError::PermissionDenied { .. } => {
                "Add a udev rule for 05e3:0144 (see README.md) and replug the scanner."
            }
            // A match arm can add a condition with `if`; the first arm whose pattern and
            // condition both match is used.
            ScanError::DriverUnavailable { reason } if reason.contains("missing") => {
                "Install libsane1."
            }
            ScanError::DriverUnavailable { .. } => {
                "Run `primefilm -vv detect` and check the model number in sane.d/pieusb.conf."
            }
            ScanError::Disconnected { .. } => "Check power and cable, then run the command again.",
            ScanError::Interrupted => "Run the command again when ready.",
        };
        text.to_string()
    }
}

// Unit tests: `#[cfg(test)]` compiles this module only for `cargo test`, so it adds nothing to
// the real program. Each `#[test]` function is one test; it passes unless it panics (which the
// `assert!` family does on failure). `use super::*` brings in everything from the file above,
// including private items, which is why unit tests live next to the code they test.
#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<ScanError> {
        vec![
            ScanError::NotFound,
            ScanError::Busy { device: None },
            ScanError::PermissionDenied {
                path: "/dev/bus/usb/001/004".into(),
            },
            ScanError::DriverUnavailable {
                reason: "driver file missing".into(),
            },
            ScanError::Disconnected {
                during: "reading options".into(),
            },
            ScanError::Interrupted,
            ScanError::Sane {
                status: 9,
                text: "Error during device I/O".into(),
            },
        ]
    }

    #[test]
    fn exit_codes_and_outcomes() {
        let pairs: Vec<(u8, &str)> = all().iter().map(|e| (e.exit_code(), e.outcome())).collect();
        assert_eq!(
            pairs,
            vec![
                (3, "not-found"),
                (4, "busy"),
                (5, "permission-denied"),
                (6, "driver-unavailable"),
                (7, "disconnected"),
                (130, "interrupted"),
                (1, "error"),
            ]
        );
    }

    #[test]
    fn exit_codes_are_distinct() {
        let mut codes: Vec<u8> = all().iter().map(ScanError::exit_code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), 7);
        assert!(!codes.contains(&0) && !codes.contains(&2));
    }

    #[test]
    fn messages_and_next_steps_are_non_empty() {
        for err in all() {
            assert!(!err.to_string().is_empty());
            assert!(!err.next_step().is_empty());
        }
        assert_eq!(
            ScanError::PermissionDenied {
                path: "/dev/bus/usb/001/004".into()
            }
            .to_string(),
            "No permission to access /dev/bus/usb/001/004."
        );
    }

    #[test]
    fn driver_next_step_depends_on_reason() {
        let missing = ScanError::DriverUnavailable {
            reason: "driver file missing".into(),
        };
        let unrecognized = ScanError::DriverUnavailable {
            reason: "scanner on USB but pieusb did not recognize it".into(),
        };
        assert!(missing.next_step().contains("libsane1"));
        assert!(unrecognized.next_step().contains("pieusb.conf"));
    }
}
