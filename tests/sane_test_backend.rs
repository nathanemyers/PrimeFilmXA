//! Runs the real SANE FFI code against SANE's bundled `test` backend (research D7), so the
//! pointer handling, struct layouts and value reading are exercised without the scanner.
//! Each test runs the binary as its own process, so SANE's global state never clashes.

// Everything in `tests/` is test code: a panic here is a failed test, which is what we want
// (constitution Principle VI allows unwrap/expect in tests). clippy only recognizes
// `#[test]` functions as tests, not their helpers, so allow it for the whole file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::process::Command;

use serde_json::Value;

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

#[test]
fn detect_lists_only_test_devices() {
    let (code, stdout, stderr) = primefilm(&["--sane-backend", "test", "--json", "detect"]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    let devices = doc["devices"].as_array().unwrap();
    assert!(!devices.is_empty());
    for d in devices {
        let name = d["name"].as_str().unwrap();
        // FR-002: only the configured backend's devices, never webcams (`v4l:`).
        assert!(name.starts_with("test:"), "{name}");
        assert!(!name.starts_with("v4l:"));
    }
}

#[test]
fn capabilities_reads_every_option_type() {
    let (code, stdout, stderr) = primefilm(&["--sane-backend", "test", "--json", "capabilities"]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    let options = doc["options"].as_array().unwrap();

    let types: HashSet<&str> = options
        .iter()
        .map(|o| o["value_type"].as_str().unwrap())
        .collect();
    for t in ["int", "fixed", "bool", "string", "button", "group"] {
        assert!(types.contains(t), "no {t} option");
    }

    for o in options {
        let t = o["value_type"].as_str().unwrap();
        if t == "button" || t == "group" {
            assert!(o["value"].is_null(), "{o}");
        }
    }

    // Arrays are read in full, not truncated to the first element (research D2).
    assert!(
        options.iter().any(|o| o["value_type"] == "int"
            && o["value"].as_array().is_some_and(|v| v.len() > 1)),
        "no int array option with more than one value"
    );

    // Strings come through intact.
    assert!(
        options
            .iter()
            .any(|o| o["name"] == "mode" && o["value"].is_string())
    );
}

#[test]
fn capabilities_human_output() {
    let (code, stdout, stderr) = primefilm(&["--sane-backend", "test", "capabilities"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Summary"));
    assert!(stdout.contains("(test:0)"), "{stdout}");
}
