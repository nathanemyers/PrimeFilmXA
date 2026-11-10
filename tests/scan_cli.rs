//! Runs the scanning commands (`scan`, `roll`, `preview`, `advance`, `presets`, `films`)
//! against the simulated film strip (`--simulate`), covering files written, numbering, film
//! movement, processing options, exit codes and JSON, without hardware (002 FR-021, 003
//! FR-022, Principle IV). Since feature 003 every frame is saved as one 16-bit gray positive,
//! `<stem>.tif`, with its settings record embedded in the TIFF, and never as a raw capture.

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
        "/specs/003-bw-processing/contracts/scan-output.schema.json"
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
    assert_eq!(doc["schema_version"], 2);
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
        for key in ["bits", "film_moved"] {
            let values = file_schema["properties"][key]["enum"].as_array().unwrap();
            assert!(values.contains(&file[key]), "{key} = {}", file[key]);
        }
        assert_eq!(
            file["channels"],
            file_schema["properties"]["channels"]["const"]
        );
        let stocks = schema["$defs"]["stock_value"]["enum"].as_array().unwrap();
        assert!(stocks.contains(&file["film"]), "film = {}", file["film"]);
    }
}

/// The pixels of a saved gray TIFF, as 16-bit values (8-bit files scaled up).
fn gray_pixels(path: &Path) -> (u32, u32, u16, Vec<u16>) {
    let mut decoder = Decoder::new(Cursor::new(fs::read(path).unwrap())).unwrap();
    let (w, h) = decoder.dimensions().unwrap();
    assert_eq!(
        decoder
            .get_tag_u32(tiff::tags::Tag::PhotometricInterpretation)
            .unwrap(),
        1,
        "black-is-zero gray"
    );
    match decoder.read_image().unwrap() {
        DecodingResult::U16(pixels) => (w, h, 16, pixels),
        DecodingResult::U8(pixels) => (
            w,
            h,
            8,
            pixels.iter().map(|&v| u16::from(v) * 257).collect(),
        ),
        _ => panic!("unexpected sample type"),
    }
}

/// The settings record embedded in a saved TIFF's `ImageDescription` tag.
fn record(path: &Path) -> Value {
    let mut decoder = Decoder::new(Cursor::new(fs::read(path).unwrap())).unwrap();
    json(
        &decoder
            .get_tag_ascii_string(tiff::tags::Tag::ImageDescription)
            .unwrap(),
    )
}

/// `names` without hidden files, which the tests check for separately.
fn visible(dir: &Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .collect()
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
    // One positive; no raw capture, no sidecar, no share copy, no hidden file.
    assert_eq!(names(&out.join("t")), vec!["t-001.tif"]);
    assert!(stdout.contains("t-001.tif"), "{stdout}");
    assert!(stderr.contains("film not moved"), "{stderr}");
    assert!(stderr.contains("saved t-001.tif ("), "{stderr}");
    assert!(!stderr.contains("advancing"), "{stderr}");

    let record = record(&out.join("t/t-001.tif"));
    assert_eq!(record["record_version"], 2);
    assert_eq!(record["preset"], "standard");
    // The capture's format; the output is described separately.
    assert_eq!(record["color_mode"], "RGBI");
    assert_eq!(record["advance_requested"], false);
    assert!(!record["options"].as_array().unwrap().is_empty());
    assert_eq!(record["output"]["kind"], "positive-master");
    assert_eq!(record["output"]["bits_per_sample"], 16);
    assert_eq!(record["processing"]["film_type"], "bw-negative");
    assert_eq!(record["processing"]["film_stock"], "bw");
    assert_eq!(record["processing"]["dust"]["level"], "off");
    assert_eq!(record["processing"]["points"]["source"], "auto");

    let (w, h, bits, pixels) = gray_pixels(&out.join("t/t-001.tif"));
    assert_eq!(
        (w, h, bits),
        (
            record["width"].as_u64().unwrap() as u32,
            record["height"].as_u64().unwrap() as u32,
            16
        )
    );
    // The clear film-base border of the negative is black in the positive.
    assert!(
        pixels[w as usize * 10] < 2000,
        "border {}",
        pixels[w as usize * 10]
    );
}

/// The simulated frames carry three opaque one-pixel specks at these positions
/// (`backend::fake::dust_specks`).
fn fake_specks(width: u32, height: u32) -> [(u32, u32); 3] {
    [
        (width / 3, height / 3),
        (width / 2, height / 2),
        (2 * width / 3, 2 * height / 3),
    ]
}

#[test]
fn dust_removal_is_off_by_default_and_only_touches_the_specks() {
    let out = temp_dir("dust");
    let scan = |extra: &[&str]| {
        let mut args = vec![
            "--simulate",
            "ready",
            "scan",
            "--roll",
            "d",
            "--output",
            s(&out),
        ];
        args.extend(extra);
        let (code, _, stderr) = primefilm(&args);
        assert_eq!(code, 0, "{stderr}");
        stderr
    };
    let stderr = scan(&[]);
    assert!(!stderr.contains("dust specks removed"), "{stderr}");
    let stderr = scan(&["--dust", "normal"]);
    assert!(stderr.contains("3 dust specks removed"), "{stderr}");
    let plain = record(&out.join("d/d-001.tif"));
    assert_eq!(plain["processing"]["dust"]["level"], "off");
    assert_eq!(plain["processing"]["dust"]["repaired"], 0);
    let cleaned = record(&out.join("d/d-002.tif"));
    assert_eq!(cleaned["processing"]["dust"]["level"], "normal");
    assert_eq!(cleaned["processing"]["dust"]["repaired"], 3);

    {
        let (w, h, _, a) = gray_pixels(&out.join("d/d-001.tif"));
        let (_, _, _, b) = gray_pixels(&out.join("d/d-002.tif"));
        let specks = fake_specks(w, h);
        let mut changed = 0;
        for (i, (pa, pb)) in a.iter().zip(&b).enumerate() {
            if pa != pb {
                changed += 1;
                let (x, y) = (i as u32 % w, i as u32 / w);
                assert!(
                    specks
                        .iter()
                        .any(|&(sx, sy)| x.abs_diff(sx) <= 5 && y.abs_diff(sy) <= 5),
                    "pixel ({x}, {y}) changed away from the specks"
                );
            }
        }
        assert!(changed >= 3, "the specks were repaired");
    }

    let (code, _, _) = primefilm(&[
        "--simulate",
        "ready",
        "scan",
        "--dust",
        "bogus",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 2);
}

#[test]
fn scan_records_the_film_stock() {
    let out = temp_dir("stock");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "scan",
        "--roll",
        "t",
        "--film",
        "hp5",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["film"], "hp5");
    assert_eq!(doc["files"][0]["film"], "hp5");
    assert!(
        doc["files"][0]["image"]
            .as_str()
            .unwrap()
            .ends_with("t-001.tif")
    );
    let record = record(&out.join("t/t-001.tif"));
    assert_eq!(record["processing"]["film_stock"], "hp5");
}

#[test]
fn film_stock_usage_errors_touch_nothing() {
    let out = temp_dir("filmerr");
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "ready",
        "scan",
        "--film",
        "color",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("Color film is not supported yet."),
        "{stderr}"
    );
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "ready",
        "roll",
        "--film",
        "tri-x",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 2);
    assert!(stderr.contains("bw, hp5, fp4"), "{stderr}");
    assert!(names(&out).is_empty());
}

#[test]
fn films_list() {
    let (code, stdout, _) = primefilm(&["films"]);
    assert_eq!(code, 0);
    for text in [
        "bw",
        "hp5",
        "Ilford HP5 Plus",
        "fp4",
        "Ilford FP4 Plus",
        "(default)",
        "Color film is not supported yet.",
    ] {
        assert!(stdout.contains(text), "{text} missing:\n{stdout}");
    }
    let (_, stdout, _) = primefilm(&["--json", "films"]);
    let doc = json(&stdout);
    assert_eq!(doc["schema_version"], 2);
    assert_eq!(doc["outcome"], "completed");
    let films = doc["films"].as_array().unwrap();
    assert_eq!(films.len(), 3);
    let film_schema = &schema()["$defs"]["film"];
    for film in films {
        for key in film_schema["required"].as_array().unwrap() {
            assert!(film.get(key.as_str().unwrap()).is_some(), "missing {key}");
        }
        assert_eq!(film["film_type"], "bw-negative");
    }
    assert_eq!(films[0]["value"], "bw");
    assert_eq!(films[0]["default"], true);
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
        vec!["t-001-2.tif", "t-001.tif", "t-002.tif"]
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
    assert_eq!(doc["files"][0]["channels"], "Gray");
    assert_eq!(doc["files"][0]["bits"], 16);
    assert_eq!(doc["film"], "bw");
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
        // Film order itself is checked on the captured frames in `src/scan.rs`.
        let moved = if frame < 6 { "advanced" } else { "none" };
        assert_eq!(file["film_moved"], moved, "frame {frame}");
    }
    // Frame 7 repeated frame 6 (the scanner stays on the last frame) and was not saved.
    assert!(
        stderr.contains("Frame 007: same as the previous frame; end of film."),
        "{stderr}"
    );
    assert_eq!(names(&out.join("r")).len(), 6);
    assert!(names(&out.join("r")).iter().all(|n| !n.starts_with('.')));
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
    assert_eq!(names(&out.join("a")).len(), 3);

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
    assert_eq!(names(&out.join("b")).len(), 2);
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
    assert_eq!(names(&out.join("e")).len(), 3);
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
        "--film",
        "fp4",
        "--dust",
        "gentle",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 9, "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["outcome"], "scan-failed");
    assert_eq!(doc["files"].as_array().unwrap().len(), 2);
    let next = doc["next_step"].as_str().unwrap();
    // The resume command repeats the processing options that were given.
    assert!(
        next.contains("primefilm roll --roll j --start-frame 3 --film fp4 --dust gentle"),
        "{next}"
    );
    for file in doc["files"].as_array().unwrap() {
        let record = record(Path::new(file["image"].as_str().unwrap()));
        assert_eq!(record["processing"]["film_stock"], "fp4");
    }
    // Only finished frames; no partial or raw file left behind.
    assert_eq!(names(&out.join("j")), vec!["j-001.tif", "j-002.tif"]);
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
    assert_eq!(names(&out.join("d")).len(), 2);
    assert!(names(&out.join("d")).iter().all(|n| !n.starts_with('.')));
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
    assert_eq!(names(&out.join("i")).len(), 2);

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
    assert_eq!(doc["files"][0]["channels"], "Gray");
    assert_eq!(doc["files"][0]["bits"], 8);
    assert!(doc["files"][0].get("frame").is_none());
    assert!(doc["files"][0].get("share_copy").is_none());
    assert_eq!(names(&out), vec!["previews"]);
    let previews = names(&out.join("previews"));
    assert_eq!(previews.len(), 1);
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
    let record = record(&out.join("q/q-001.tif"));
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

// ---- Feature 003: tone balancing is left to post-processing ----

#[test]
fn tone_options_are_gone() {
    let out = temp_dir("notones");
    for extra in [
        vec!["--brightness", "10"],
        vec!["--contrast", "10"],
        vec!["--black", "0.2", "--white", "1.2"],
    ] {
        let mut args = vec!["--simulate", "ready", "scan", "--output", s(&out)];
        args.extend(&extra);
        let (code, _, stderr) = primefilm(&args);
        assert_eq!(code, 2, "{extra:?}: {stderr}");
    }
    let (code, _, _) = primefilm(&["--simulate", "ready", "roll", "--match-tones"]);
    assert_eq!(code, 2);
    assert!(names(&out).is_empty());
}

// ---- Feature 003, user story 6: positive previews ----

#[test]
fn previews_are_8_bit_positives_without_dust_removal() {
    let out = temp_dir("previewpos");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "preview",
        "--film",
        "hp5",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let doc = json(&stdout);
    let file = &doc["files"][0];
    let (_, _, bits, _) = gray_pixels(Path::new(file["image"].as_str().unwrap()));
    assert_eq!(bits, 8);
    let record = record(Path::new(file["image"].as_str().unwrap()));
    assert_eq!(record["output"]["kind"], "preview");
    assert_eq!(record["processing"]["film_stock"], "hp5");
    assert_eq!(record["processing"]["dust"]["level"], "off");
    assert!(
        names(&out.join("previews"))
            .iter()
            .all(|n| n.ends_with(".tif"))
    );

    let (code, _, _) = primefilm(&[
        "--simulate",
        "ready",
        "preview",
        "--dust",
        "normal",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 2, "previews take no --dust");
}

// ---- Feature 003: development-only raw captures ----

#[test]
fn keep_capture_writes_raw_captures_only_where_asked() {
    let out = temp_dir("keep");
    let keep = out.join("captures");
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "ready",
        "--keep-capture",
        s(&keep),
        "roll",
        "--roll",
        "k",
        "--frames",
        "2",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(names(&keep), vec!["k-001-capture.tif", "k-002-capture.tif"]);
    assert!(
        visible(&out.join("k"))
            .iter()
            .all(|n| !n.contains("capture"))
    );
    let record = record(&out.join("k/k-001.tif"));
    assert!(
        record["processing"]["kept_capture"]
            .as_str()
            .unwrap()
            .ends_with("k-001-capture.tif")
    );
    // The kept capture is the raw negative: 4 samples (RGBI) of 16 bits.
    let mut decoder = Decoder::new(Cursor::new(
        fs::read(keep.join("k-001-capture.tif")).unwrap(),
    ))
    .unwrap();
    assert_eq!(
        decoder
            .get_tag_u32(tiff::tags::Tag::SamplesPerPixel)
            .unwrap(),
        4
    );
}

// ---- Roll preview ----

#[test]
fn roll_preview_scans_the_whole_roll_into_previews() {
    let out = temp_dir("rollpreview");
    let (code, stdout, stderr) = primefilm(&[
        "--json",
        "--simulate",
        "ready",
        "--simulate-frames",
        "3",
        "roll",
        "--preview",
        "--roll",
        "p",
        "--film",
        "hp5",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("(preview, 300 dpi"), "{stderr}");
    let doc = json(&stdout);
    check_schema(&doc);
    assert_eq!(doc["outcome"], "end-of-film");
    assert!(doc.get("preset").is_none());
    let files = doc["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(files[0]["film_moved"], "advanced");
    assert_eq!(files[0]["bits"], 8);
    // Kept apart from archival scans: nothing outside previews/.
    assert_eq!(names(&out), vec!["previews"]);
    assert_eq!(
        names(&out.join("previews/p")),
        vec!["p-001.tif", "p-002.tif", "p-003.tif"]
    );
    let record = record(&out.join("previews/p/p-001.tif"));
    assert_eq!(record["output"]["kind"], "preview");
    assert_eq!(record["preset"], "preview");
    assert_eq!(record["advance_requested"], true);
    assert_eq!(record["processing"]["dust"]["level"], "off");
    assert_eq!(record["processing"]["film_stock"], "hp5");
}

#[test]
fn interrupted_roll_preview_resumes_as_a_preview() {
    let out = temp_dir("rollpreviewresume");
    let (code, _, stderr) = primefilm(&[
        "--simulate",
        "interrupt",
        "roll",
        "--preview",
        "--roll",
        "p",
        "--output",
        s(&out),
    ]);
    assert_eq!(code, 130);
    assert!(
        stderr.contains("primefilm roll --roll p --start-frame 3 --preview"),
        "{stderr}"
    );
    assert!(!stderr.contains("--dust"), "{stderr}");
}

#[test]
fn roll_preview_rejects_quality_and_dust_options() {
    let out = temp_dir("rollpreviewerr");
    for extra in [
        vec!["--preset", "web"],
        vec!["--resolution", "2400"],
        vec!["--dust", "normal"],
    ] {
        let mut args = vec![
            "--simulate",
            "ready",
            "roll",
            "--preview",
            "--output",
            s(&out),
        ];
        args.extend(&extra);
        let (code, _, stderr) = primefilm(&args);
        assert_eq!(code, 2, "{extra:?}: {stderr}");
    }
    assert!(names(&out).is_empty());
}
