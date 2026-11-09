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

/// A fresh output folder for a hardware test.
fn temp_out(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("primefilm-hw-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The file names in a folder, hidden ones included, sorted.
fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Needs a B&W strip loaded. Does not move the film. The preview is a positive (003 US6).
#[test]
#[ignore]
fn preview_is_quick() {
    let out = temp_out("preview");
    let started = std::time::Instant::now();
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "preview",
        "--film",
        "hp5",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["files"][0]["channels"], "Gray");
    assert_eq!(doc["files"][0]["bits"], 8);
    // SC-002 (the lock wait is negligible: tests take turns only on contention).
    assert!(started.elapsed().as_secs() < 15, "{:?}", started.elapsed());
}

/// Needs an HP5 strip loaded. Does not move the film. Saves a positive, a share copy and a
/// record, and no raw capture (003 US1).
#[test]
#[ignore]
fn scan_one_frame() {
    let out = temp_out("scan");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "scan",
        "--roll",
        "hwtest",
        "--film",
        "hp5",
        "--preset",
        "web",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["files"][0]["channels"], "Gray");
    assert_eq!(doc["files"][0]["bits"], 16);
    assert_eq!(
        names(&out.join("hwtest")),
        vec!["hwtest-001.jpg", "hwtest-001.json", "hwtest-001.tif"]
    );
}

/// Needs an FP4 strip with at least 2 frames loaded. MOVES THE FILM two frames.
#[test]
#[ignore]
fn roll_two_frames() {
    let out = temp_out("roll");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "roll",
        "--roll",
        "hwtest",
        "--film",
        "fp4",
        "--preset",
        "web",
        "--frames",
        "2",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 2);
    assert_eq!(
        names(&out.join("hwtest")).len(),
        6,
        "3 files per frame, nothing else"
    );
}
