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

/// A real scan through the FFI scan path (`sane_start`, `sane_get_parameters`, `sane_read`,
/// `sane_cancel`, option setting) against SANE's test scanner, checked by decoding the TIFF.
/// Since feature 003 the frame is saved as a 16-bit gray positive with a version 2 settings
/// record in its `ImageDescription` tag.
#[test]
fn scan_writes_a_decodable_tiff() {
    let out = std::env::temp_dir().join(format!("primefilm-sane-test-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let (code, stdout, stderr) = primefilm(&[
        "--sane-backend",
        "test",
        "--json",
        "scan",
        "--roll",
        "t",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    let file = &doc["files"][0];

    let bytes = std::fs::read(file["image"].as_str().unwrap()).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).unwrap();
    let (width, height) = decoder.dimensions().unwrap();
    assert_eq!(u64::from(width), file["width"].as_u64().unwrap());
    assert_eq!(u64::from(height), file["height"].as_u64().unwrap());
    assert!(width > 1 && height > 1, "{width} × {height}");
    assert_eq!(
        decoder
            .get_tag_u32(tiff::tags::Tag::SamplesPerPixel)
            .unwrap(),
        1
    );
    let description = decoder
        .get_tag_ascii_string(tiff::tags::Tag::ImageDescription)
        .unwrap();
    assert!(matches!(
        decoder.read_image().unwrap(),
        tiff::decoder::DecodingResult::U16(_)
    ));

    let record: Value = serde_json::from_str(&description).unwrap();
    assert!(
        record["scanner"]["name"]
            .as_str()
            .unwrap()
            .starts_with("test:")
    );
    assert!(!record["options"].as_array().unwrap().is_empty());
    assert_eq!(record["record_version"], 2);
    assert_eq!(record["output"]["kind"], "positive-master");
    let _ = std::fs::remove_dir_all(&out);
}
