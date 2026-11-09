//! Needs the real PrimeFilm XA. Run with `cargo test -- --ignored`.
//!
//! `#[ignore]` keeps these out of a normal `cargo test`, so the test suite passes with the
//! scanner off and never touches the hardware unless asked (constitution Principle IV).

// Everything in `tests/` is test code: a panic here is a failed test, which is what we want
// (constitution Principle VI allows unwrap/expect in tests). clippy only recognizes
// `#[test]` functions as tests, not their helpers, so allow it for the whole file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;
use std::sync::Mutex;

use serde_json::Value;

/// There is one physical scanner, but `cargo test` runs tests in parallel. Each test holds
/// this lock while it runs `primefilm`, so they take turns instead of fighting over the USB
/// interface. A `Mutex` ("mutual exclusion") lets only one thread hold it at a time.
static SCANNER: Mutex<()> = Mutex::new(());

fn primefilm(args: &[&str]) -> (i32, String, String) {
    // If an earlier test panicked while holding the lock, the lock is "poisoned"; the scanner
    // itself is fine, so take the lock anyway rather than failing every later test.
    let _turn = SCANNER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let out = Command::new(env!("CARGO_BIN_EXE_primefilm"))
        .args(args)
        .output()
        .expect("failed to run primefilm");
    (
        out.status.code().expect("primefilm was killed by a signal"),
        String::from_utf8(out.stdout).expect("stdout is UTF-8"),
        String::from_utf8(out.stderr).expect("stderr is UTF-8"),
    )
}

#[test]
#[ignore]
fn detect_finds_scanner() {
    let (code, stdout, stderr) = primefilm(&["detect"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("pieusb:"), "{stdout}");
}

#[test]
#[ignore]
fn capabilities_lists_options() {
    let (code, stdout, stderr) = primefilm(&["--json", "capabilities"]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert!(!doc["options"].as_array().unwrap().is_empty());
    assert!(!doc["summary"]["resolutions_dpi"].is_null());
}
