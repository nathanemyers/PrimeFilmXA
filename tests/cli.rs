//! Runs the real `primefilm` binary against the simulated scanner (`--simulate`), covering
//! every outcome, exit code and JSON shape without hardware (FR-011, Principle IV).
//!
//! Integration tests live in `tests/` and are compiled as separate programs; they can only
//! use the binary and the library's public API, just like a user or another crate.

// Everything in `tests/` is test code: a panic here is a failed test, which is what we want
// (constitution Principle VI allows unwrap/expect in tests). clippy only recognizes
// `#[test]` functions as tests, not their helpers, so allow it for the whole file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use serde_json::Value;

/// Run `primefilm` with `args` and return (exit code, stdout, stderr).
fn primefilm(args: &[&str]) -> (i32, String, String) {
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

fn json(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{stdout}"))
}

// ---- User story 1: detect ----

#[test]
fn detect_ready() {
    let (code, stdout, _) = primefilm(&["--simulate", "ready", "detect"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("MF Scanner"), "{stdout}");
}

#[test]
fn detect_no_scanner() {
    let (code, _, stderr) = primefilm(&["--simulate", "no-scanner", "detect"]);
    assert_eq!(code, 3);
    assert!(stderr.contains("No PrimeFilm XA found"), "{stderr}");
}

#[test]
fn detect_ready_json() {
    let (code, stdout, _) = primefilm(&["--json", "--simulate", "ready", "detect"]);
    assert_eq!(code, 0);
    let doc = json(&stdout);
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["outcome"], "found");
    let devices = doc["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 1);
    for key in ["name", "vendor", "model", "kind"] {
        assert!(devices[0][key].is_string(), "missing {key}");
    }
}

#[test]
fn detect_no_scanner_json() {
    let (code, stdout, _) = primefilm(&["--json", "--simulate", "no-scanner", "detect"]);
    assert_eq!(code, 3);
    let doc = json(&stdout);
    assert_eq!(doc["outcome"], "not-found");
    assert!(!doc["message"].as_str().unwrap().is_empty());
    assert!(!doc["next_step"].as_str().unwrap().is_empty());
}

// ---- User story 2: capabilities ----

#[test]
fn capabilities_ready() {
    let (code, stdout, _) = primefilm(&["--simulate", "ready", "capabilities"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Summary"), "{stdout}");
    assert!(stdout.contains("Infrared"), "{stdout}");
}

#[test]
fn capabilities_ready_json() {
    let (code, stdout, _) = primefilm(&["--json", "--simulate", "ready", "capabilities"]);
    assert_eq!(code, 0);
    let doc = json(&stdout);
    assert_eq!(doc["outcome"], "found");
    assert!(doc["device"].is_object());
    assert!(doc["summary"].is_object());
    let options = doc["options"].as_array().unwrap();
    assert!(!options.is_empty());

    let keys = [
        "index",
        "name",
        "title",
        "description",
        "value_type",
        "unit",
        "size",
        "active",
        "settable",
        "automatic",
        "advanced",
        "constraint",
        "value",
    ];
    for opt in options {
        let obj = opt.as_object().unwrap();
        assert_eq!(obj.len(), keys.len(), "unexpected keys in {opt}");
        for key in keys {
            assert!(obj.contains_key(key), "missing {key} in {opt}");
        }
    }
    // US2 scenario 3: inactive options are listed, with no value.
    assert!(
        options
            .iter()
            .any(|o| o["active"] == false && o["value"].is_null())
    );
}

#[test]
fn capabilities_unknown_device() {
    let (code, _, _) = primefilm(&["--simulate", "ready", "capabilities", "--device", "nope"]);
    assert_eq!(code, 3);
}

// ---- User story 3: failure outcomes ----

/// (scenario, command, exit code, JSON outcome)
const FAILURES: [(&str, &str, i32, &str); 6] = [
    ("no-scanner", "detect", 3, "not-found"),
    ("busy", "detect", 4, "busy"),
    ("permission-denied", "detect", 5, "permission-denied"),
    ("driver-missing", "detect", 6, "driver-unavailable"),
    ("disconnect", "capabilities", 7, "disconnected"),
    ("interrupt", "capabilities", 130, "interrupted"),
];

#[test]
fn failure_exit_codes() {
    for (scenario, command, expected, _) in FAILURES {
        let (code, _, stderr) = primefilm(&["--simulate", scenario, command]);
        assert_eq!(code, expected, "{scenario}: {stderr}");
        assert!(stderr.contains("Next step:"), "{scenario}: {stderr}");
    }
}

#[test]
fn busy_message() {
    let (_, _, stderr) = primefilm(&["--simulate", "busy", "detect"]);
    assert!(stderr.contains("in use"), "{stderr}");
}

#[test]
fn failure_json() {
    for (scenario, command, _, outcome) in FAILURES {
        let (_, stdout, _) = primefilm(&["--json", "--simulate", scenario, command]);
        let doc = json(&stdout);
        assert_eq!(doc["schema_version"], 1, "{scenario}");
        assert_eq!(doc["outcome"], outcome, "{scenario}");
        assert!(!doc["message"].as_str().unwrap().is_empty(), "{scenario}");
        assert!(!doc["next_step"].as_str().unwrap().is_empty(), "{scenario}");
    }
}

#[test]
fn failure_exit_codes_are_distinct() {
    // SC-003: a script can tell every outcome apart by exit code alone.
    let mut codes: Vec<i32> = FAILURES.iter().map(|f| f.2).collect();
    codes.sort();
    codes.dedup();
    assert_eq!(codes.len(), FAILURES.len());
    assert!(!codes.contains(&0) && !codes.contains(&2));
}

#[test]
fn verbose_shows_steps() {
    let (code, _, stderr) = primefilm(&["-v", "--simulate", "ready", "detect"]);
    assert_eq!(code, 0);
    assert!(stderr.contains("primefilm:"), "{stderr}");
}

#[test]
fn quiet_by_default() {
    let (_, _, stderr) = primefilm(&["--simulate", "ready", "detect"]);
    assert!(!stderr.contains("primefilm:"), "{stderr}");
}

#[test]
fn bad_arguments_exit_2() {
    let (code, stdout, _) = primefilm(&["--json", "frobnicate"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
}
