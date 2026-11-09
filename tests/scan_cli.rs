//! Runs the scanning commands (`scan`, `roll`, `preview`, `advance`, `presets`) against the
//! simulated film strip (`--simulate`), covering files written, numbering, film movement, exit
//! codes and JSON, without hardware (FR-021, Principle IV).

// Everything in `tests/` is test code: a panic here is a failed test, which is what we want
// (constitution Principle VI allows unwrap/expect in tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use tiff::decoder::{Decoder, DecodingResult};

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

/// A fresh, empty output folder for one test. Integration tests run in parallel, so each gets
/// its own directory under the system temp dir.
fn temp_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "primefilm-scan-cli-{}-{label}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Every file name in `dir`, hidden ones included, sorted.
fn names(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn json(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{stdout}"))
}

/// The JSON schema from the contract.
fn schema() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/specs/002-film-scan/contracts/scan-output.schema.json"
    );
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// Check `doc` against the parts of the schema that matter: required fields, the allowed
/// outcomes and presets, and every file entry's required fields and allowed values.
fn check_schema(doc: &Value) {
    let schema = schema();
    for key in schema["required"].as_array().unwrap() {
        assert!(
            doc.get(key.as_str().unwrap()).is_some(),
            "missing {key} in {doc}"
        );
    }
    assert_eq!(doc["schema_version"], 1);
    let allowed = |prop: &str, value: &Value| {
        schema["properties"][prop]["enum"]
            .as_array()
            .unwrap()
            .contains(value)
    };
    assert!(
        allowed("outcome", &doc["outcome"]),
        "outcome {}",
        doc["outcome"]
    );
    if let Some(preset) = doc.get("preset") {
        assert!(allowed("preset", preset), "preset {preset}");
    }
    let file_schema = &schema["$defs"]["file"];
    for file in doc["files"].as_array().into_iter().flatten() {
        for key in file_schema["required"].as_array().unwrap() {
            assert!(
                file.get(key.as_str().unwrap()).is_some(),
                "missing {key} in {file}"
            );
        }
        for key in ["channels", "bits", "film_moved"] {
            let values = file_schema["properties"][key]["enum"].as_array().unwrap();
            assert!(values.contains(&file[key]), "{key} = {}", file[key]);
        }
    }
}

/// The first red sample of a saved TIFF: the simulated scanner stores the frame number there.
fn first_sample(path: &Path) -> u32 {
    let mut decoder = Decoder::new(Cursor::new(fs::read(path).unwrap())).unwrap();
    match decoder.read_image().unwrap() {
        DecodingResult::U16(pixels) => u32::from(pixels[0]),
        DecodingResult::U8(pixels) => u32::from(pixels[0]),
        _ => panic!("unexpected sample type"),
    }
}

// ---- User story 2: scan a single frame ----

#[test]
fn scan_saves_one_frame_without_moving_film() {
    let out = temp_dir("scan");
    let (code, stdout, stderr) = primefilm(&[
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "t",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(names(&out.join("t")), vec!["t-001.json", "t-001.tif"]);
    assert!(stdout.contains("t-001.tif"), "{stdout}");
    assert!(stderr.contains("film not moved"), "{stderr}");
    assert!(!stderr.contains("advancing"), "{stderr}");

    let record = json(&fs::read_to_string(out.join("t/t-001.json")).unwrap());
    assert_eq!(record["preset"], "standard");
    assert_eq!(record["color_mode"], "RGBI");
    assert_eq!(record["bits_per_sample"], 16);
    assert_eq!(record["advance_requested"], false);
    assert!(!record["options"].as_array().unwrap().is_empty());
}

#[test]
fn scan_numbering_never_overwrites() {
    let out = temp_dir("numbering");
    let args = [
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "t",
        "--output",
        s(&out),
    ];
    assert_eq!(primefilm(&args).0, 0);
    assert_eq!(primefilm(&args).0, 0);
    let original = fs::read(out.join("t/t-001.tif")).unwrap();
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "t",
        "--frame",
        "1",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        names(&out.join("t")),
        vec![
            "t-001-2.json",
            "t-001-2.tif",
            "t-001.json",
            "t-001.tif",
            "t-002.json",
            "t-002.tif"
        ]
    );
    assert_eq!(fs::read(out.join("t/t-001.tif")).unwrap(), original);
}

#[test]
fn scan_with_no_film() {
    let out = temp_dir("nofilm");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "no-film",
        "scan",
        "--roll",
        "t",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 8, "{stderr}");
    assert!(stderr.contains("No film found"), "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["outcome"], "no-film");
    assert!(
        names(&out.join("t")).is_empty(),
        "no file, not even a partial"
    );
}

#[test]
fn scan_busy_uses_the_failure_document() {
    let out = temp_dir("busy");
    let (code, stdout, _) =
        primefilm(&["--json", "--simulate", "busy", "scan", "--output", s(&out)]);
    assert_eq!(code, 4);
    let doc = json(&stdout);
    check_schema(&doc);
    let mut keys: Vec<&str> = doc
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["message", "next_step", "outcome", "schema_version"]
    );
}

#[test]
fn scan_json() {
    let out = temp_dir("scanjson");
    let (code, stdout, _) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "t",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0);
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["outcome"], "completed");
    assert_eq!(doc["roll"], "t");
    assert_eq!(doc["files"][0]["frame"], 1);
    assert_eq!(doc["files"][0]["film_moved"], "none");
    assert_eq!(doc["files"][0]["channels"], "RGBI");
}

#[test]
fn scan_rejects_bad_roll_names_before_touching_the_scanner() {
    let out = temp_dir("badroll");
    let (code, stdout, _) = primefilm(&[
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "a/b",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(names(&out).is_empty());
}

// ---- User story 1: scan a whole strip ----

#[test]
fn roll_scans_until_the_film_ends() {
    let out = temp_dir("roll");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "roll",
        "--roll",
        "r",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["outcome"], "end-of-film");
    let files = doc["files"].as_array().unwrap();
    assert_eq!(files.len(), 6);
    for (i, file) in files.iter().enumerate() {
        let frame = i as u32 + 1;
        assert_eq!(file["frame"], frame);
        // In film order: each file holds the frame it is named after.
        assert_eq!(
            first_sample(Path::new(file["image"].as_str().unwrap())),
            frame
        );
        let moved = if frame < 6 { "advanced" } else { "none" };
        assert_eq!(file["film_moved"], moved, "frame {frame}");
    }
    // Frame 7 repeated frame 6 (the scanner stays on the last frame) and was not saved.
    assert!(
        stderr.contains("Frame 007: same as the previous frame; end of film."),
        "{stderr}"
    );
    assert_eq!(names(&out.join("r")).len(), 12);
}

#[test]
fn roll_frame_limits() {
    let out = temp_dir("limits");
    let (code, _, _) = primefilm(&[
        "--simulate",
        "ready",
        "--simulate-frames",
        "3",
        "roll",
        "--roll",
        "a",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0);
    assert_eq!(names(&out.join("a")).len(), 6);

    let (code, stdout, _) = primefilm(&[
        "--simulate",
        "ready",
        "roll",
        "--roll",
        "b",
        "--frames",
        "2",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Saved 2 frames (001–002)"), "{stdout}");
    assert_eq!(names(&out.join("b")).len(), 4);
}

#[test]
fn roll_ends_at_an_empty_gate() {
    let out = temp_dir("emptygate");
    let (code, stdout, _) = primefilm(&[
        "--simulate",
        "empty-gate",
        "roll",
        "--roll",
        "e",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("after frame 003"), "{stdout}");
    assert_eq!(names(&out.join("e")).len(), 6);
}

#[test]
fn roll_jam_keeps_saved_frames_and_says_how_to_resume() {
    let out = temp_dir("jam");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "jam",
        "roll",
        "--roll",
        "j",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 9, "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["outcome"], "scan-failed");
    assert_eq!(doc["files"].as_array().unwrap().len(), 2);
    let next = doc["next_step"].as_str().unwrap();
    assert!(
        next.contains("primefilm roll --roll j --start-frame 3"),
        "{next}"
    );
    // Only finished frames; no partial file left behind.
    assert_eq!(
        names(&out.join("j")),
        vec!["j-001.json", "j-001.tif", "j-002.json", "j-002.tif"]
    );
}

#[test]
fn roll_disconnect() {
    let out = temp_dir("disconnect");
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "disconnect",
        "roll",
        "--roll",
        "d",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 7);
    assert!(
        stderr.contains("disconnected while scanning frame 003"),
        "{stderr}"
    );
    assert!(stderr.contains("where the film is"), "{stderr}");
    assert_eq!(names(&out.join("d")).len(), 4);
}

#[test]
fn roll_interrupt_saves_the_frame_in_progress_then_resumes() {
    let out = temp_dir("interrupt");
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "interrupt",
        "roll",
        "--roll",
        "i",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 130);
    assert!(stderr.contains("Interrupted after frame 002"), "{stderr}");
    assert!(stderr.contains("--start-frame 3"), "{stderr}");
    assert_eq!(names(&out.join("i")).len(), 4);

    // Resuming continues the numbering without being told the start frame.
    let (code, _, _) = primefilm(&[
        "--simulate",
        "ready",
        "roll",
        "--roll",
        "i",
        "--frames",
        "2",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0);
    let tifs: Vec<String> = names(&out.join("i"))
        .into_iter()
        .filter(|n| n.ends_with(".tif"))
        .collect();
    assert_eq!(
        tifs,
        vec!["i-001.tif", "i-002.tif", "i-003.tif", "i-004.tif"]
    );
}

#[test]
fn roll_with_no_film() {
    let out = temp_dir("rollnofilm");
    let (code, _, _) = primefilm(&[
        "--simulate",
        "no-film",
        "roll",
        "--roll",
        "n",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 8);
    assert!(names(&out.join("n")).is_empty());
}

// ---- User story 3: preview ----

#[test]
fn preview_goes_to_its_own_folder() {
    let out = temp_dir("preview");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "preview",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["files"][0]["channels"], "RGB");
    assert_eq!(doc["files"][0]["bits"], 8);
    assert!(doc["files"][0].get("frame").is_none());
    assert_eq!(names(&out), vec!["previews"]);
    let previews = names(&out.join("previews"));
    assert_eq!(previews.len(), 2);
    assert!(
        previews.iter().all(|n| n.starts_with("preview-")),
        "{previews:?}"
    );
    assert!(!stderr.contains("advancing"), "{stderr}");
}

#[test]
fn preview_with_no_film() {
    let out = temp_dir("previewnofilm");
    let (code, _, _) = primefilm(&["--simulate", "no-film", "preview", "--output", s(&out)]);
    assert_eq!(code, 8);
    assert!(names(&out.join("previews")).is_empty());
}

// ---- User story 4: presets ----

#[test]
fn presets_list() {
    let (code, stdout, _) = primefilm(&["presets"]);
    assert_eq!(code, 0);
    for line in [
        "web",
        "standard",
        "high",
        "5173 × 3443",
        "(default)",
        "about 1 min 31 s",
    ] {
        assert!(stdout.contains(line), "{line} missing:\n{stdout}");
    }
    let (_, stdout, _) = primefilm(&["--json", "presets"]);
    let doc = json(&stdout);
    let presets = doc["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 3);
    let preset_schema = &schema()["$defs"]["preset"];
    for p in presets {
        for key in preset_schema["required"].as_array().unwrap() {
            assert!(p.get(key.as_str().unwrap()).is_some(), "missing {key}");
        }
    }
    let defaults: Vec<&Value> = presets.iter().filter(|p| p["default"] == true).collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0]["name"], "standard");
    assert_eq!(presets[0]["width"], 2586);
}

#[test]
fn chosen_quality_is_recorded() {
    let out = temp_dir("quality");
    let (_, stdout, _) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "q",
        "--preset",
        "web",
        "--output",
        s(&out),
    ]);
    let doc = json(&stdout);
    assert_eq!(doc["preset"], "web");
    assert_eq!(doc["resolution_dpi"], 1800.0);
    let record = json(&fs::read_to_string(out.join("q/q-001.json")).unwrap());
    assert_eq!(record["preset"], "web");
    // The simulated scanner scans at 1/50 of the resolution.
    assert_eq!(record["width"], 51);

    let (_, stdout, _) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "q",
        "--resolution",
        "2400",
        "--output",
        s(&out),
    ]);
    let doc = json(&stdout);
    assert_eq!(doc["preset"], "custom");
    assert_eq!(doc["resolution_dpi"], 2400.0);
}

#[test]
fn quality_usage_errors_touch_nothing() {
    let out = temp_dir("qualityerr");
    for extra in [
        vec!["--preset", "web", "--resolution", "2400"],
        vec!["--preset", "huge"],
        vec!["--resolution", "30000"],
        vec!["--resolution", "10000"],
        vec!["--preset", "max"],
    ] {
        let mut args = vec!["--simulate", "ready", "scan", "--output", s(&out)];
        args.extend(&extra);
        let (code, _, stderr) = primefilm(&args);
        assert_eq!(code, 2, "{extra:?}: {stderr}");
    }
    let (_, _, stderr) = primefilm(&["scan", "--preset", "huge"]);
    assert!(stderr.contains("standard"), "lists valid presets: {stderr}");
    assert!(names(&out).is_empty());
}

// ---- User story 5: advance ----

#[test]
fn advance_writes_nothing() {
    let (code, stdout, stderr) = primefilm(&["--simulate", "ready", "advance"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Film advanced one frame."), "{stdout}");

    let (code, stdout, _) = primefilm(&["--json", "--simulate", "ready", "advance"]);
    assert_eq!(code, 0);
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["film_moved"], "advanced");
    assert!(doc["files"].as_array().unwrap().is_empty());

    let (code, _, _) = primefilm(&["--simulate", "no-film", "advance"]);
    assert_eq!(code, 8);
}
