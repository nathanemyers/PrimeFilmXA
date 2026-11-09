# Implementation Plan: Black-and-White Negative Processing

**Branch**: `003-bw-processing` | **Date**: 2026-11-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/003-bw-processing/spec.md`

## Summary

`scan`, `roll` and `preview` stop saving raw negatives and save finished B&W positives
instead. While a frame streams in from the scanner, the R, G and B samples are averaged into a
16-bit gray frame held in memory (36 MB at `standard`). When the frame ends:

1. Black and white points are set automatically from a histogram of the picture area, working
   in film density.
2. Dust is removed. Opaque specks are denser than any silver on the frame, so they can be found
   without the infrared channel, which is useless on silver film.
3. One lookup table applies inversion, contrast and brightness to every pixel.

Each frame is saved three times: a 16-bit gray TIFF master, a full-resolution 8-bit JPEG share
copy, and a version-2 JSON record of the capture and every processing setting.

New options are `--film bw|hp5|fp4`, `--brightness`, `--contrast`, `--black/--white`,
`--dust off|gentle|normal|strong` and `roll --match-tones`. A new `films` command lists the
stocks. Details are in [research.md](research.md) P1–P14.

## Technical Context

**Language/Version**: Rust stable 1.99 (edition 2024), as in features 001–002

**Primary Dependencies**: existing `clap` 4, `serde` + `serde_json`, `thiserror` 2, `ctrlc` 3,
`jiff` 0.2; new `jpeg-encoder` 0.6 for the share copy (research P13). Dev-only: existing
`tiff` 0.11, now also used by `examples/process_capture.rs`.

**Storage**: files. Per frame, `<roll>-NNN.tif` (16-bit gray), `.jpg` (8-bit gray) and `.json`
(record v2) in `<output>/<roll>/`. Previews are 8-bit gray TIFF + JSON in `<output>/previews/`.

**Testing**: `cargo test`:
- unit tests on synthetic negatives (step wedge, dust, star field, blank frame, border);
- determinism;
- tone matching;
- CLI tests against the fake film strip, with JSON schema v2;
- `#[ignore]` hardware tests for HP5 and FP4.

**Target Platform**: Ubuntu Linux x86_64, normal user, PrimeFilm XA on USB

**Project Type**: CLI tool (single crate, library + binary)

**Performance Goals**: processing adds at most 20% to scan time at `standard`, which is
≤ 18 s on top of 002's 91 s (SC-002). Expected is a few seconds: every pass is linear in pixels,
and the dust search only floods candidate pixels. Disk use at `standard` is ≤ one third of the
raw size (≈ 40 MB vs 143 MB, SC-003).

**Constraints**:
- No raw data on disk (FR-014).
- Our memory is bounded per frame: gray buffer plus dust mask, ≤ about 105 MB at `high`.
- No overwrite, no complete-looking partial files, master published last (FR-018).
- Identical output for identical input and settings (FR-007).
- All option checks happen before the scanner opens (FR-020).

**Scale/Scope**: three stocks with one shared profile, four dust levels, about 1,500 new lines
of Rust including comments and tests. About 300 lines of 002's tests are adjusted for the new
outputs.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Checked against constitution **v2.0.0** (Principle I amended 2026-11-09 for this feature).

| Principle | Status | How this plan complies |
|-----------|--------|------------------------|
| I. Archival Fidelity | ✅ | The spec explicitly makes B&W negatives a processed-master film type. The master is full resolution, lossless, 16-bit. The raw capture never reaches disk: it is held in memory only, and the hidden development flag `--keep-capture` is the only exception (fixtures, research P12). The JPEG is a separate derivative with its own extension. The record keeps 002's capture settings and adds stock, profile, points (with source), brightness, contrast, dust constants and repair count, and tool version. Files are published from `.partial` by hard link, record → JPEG → master. Nothing is ever overwritten. No film type can produce a processed master without a spec: `color` is rejected before scanning. |
| II. Hardware Safety | ✅ | Scanner handling is unchanged from 002: options are per session, film moves only for `roll`/`advance`, the scanner is released on every path, and there are no retries. The only new scanner setting is a capture mode the backend already supports (`Color` 16-bit, experiment E9). |
| III. Swappable Backend | ✅ | Processing works only on the bytes a `ScanSession` returns. The backend change is one new `ColorFormat` variant. No processing code touches SANE. Experiments E9–E11 are recorded in this feature's `hardware-validation.md`. |
| IV. Testable Without Hardware | ✅ | All processing is exercised by synthetic negatives and the fake film strip, whose frames become negative-like with specks. Hardware tests are `#[ignore]`d. An example program replays real captures for tuning. |
| V. Simple, Scriptable First | ✅ | New behavior is set with flags and checked before the scanner opens. JSON v2 output exists for every command, including `films`. There are no configuration files. Dust levels are four named values and profiles are fixed in code. There is no plugin or trait layer for one film type. Color is a future enum variant (YAGNI). |
| VI. Learning by Building | ✅ | The histogram, tone table, flood fill and repair are our own commented code. One new crate (`jpeg-encoder`) is justified for file-format plumbing only. Rust concepts are listed below. There is no new `unsafe`, generics or lifetimes. |

**Platform & Workflow**: Linux only, no root, crates.io only ✅. Hardware-touching tasks
(E9–E11, quickstart steps 3–12) are verified on the scanner and recorded in
`hardware-validation.md` ✅.

**Post-design re-check (after Phase 1)**: still passing. The only deviation is holding a whole
gray frame in memory, which is justified under Complexity Tracking.

## Rust Concepts Introduced

These build on the lists in 001 and 002. Each gets an explanatory comment where it first
appears.

1. A flat `Vec<u16>` as a 2-D image: row-major indexing `y * width + x`, `chunks_exact` for rows
2. Lookup tables built with iterators: `(0..=u16::MAX).map(…).collect::<Vec<u16>>()`
3. Floating-point maths: `log10`, `powf`, `clamp`, `round`; why the same inputs always give the
   same result on one machine (FR-007)
4. Cumulative sums over a histogram to find percentiles: `iter().scan` or a running total in a
   `for` loop
5. Iteration without recursion: an explicit stack with `while let Some(p) = stack.pop()` (flood
   fill), and why deep recursion would overflow the stack on a large speck
6. Checked integer conversions for pixel counts and indices: `usize::try_from`, and
   `saturating_sub` at image edges
7. clap value parsers with ranges and custom validation (`value_parser!(i32).range(-50..=50)`,
   a parser function returning `Result<f64, String>`)
8. `impl Default` and plain data structs for profiles; `match` arms per enum variant as the
   place where future stocks and types plug in
9. `examples/`: small programs that use the library, built with `cargo run --example`
10. Using a third-party encoder through its builder-style API, and turning its error type into
    ours with `map_err`

## Project Structure

### Documentation (this feature)

```text
specs/003-bw-processing/
├── plan.md                    # This file
├── research.md                # Decisions P1–P14, experiments E9–E11, risks
├── data-model.md              # Film types/stocks, profile, settings, points, gray frame, record v2
├── quickstart.md              # Simulated checks and hardware validation steps
├── contracts/
│   ├── cli.md                 # Processing options, films command, files, output
│   └── scan-output.schema.json   # JSON schema version 2
├── checklists/requirements.md
├── hardware-validation.md     # Filled in during implementation (E9–E11, SC-001/002/003/006)
└── tasks.md                   # Created by /speckit-tasks
```

### Source Code (repository root)

```text
Cargo.toml                     # CHANGED: + jpeg-encoder
src/
├── lib.rs                     # CHANGED: dispatch `films`, pass ProcessSettings to scan commands
├── cli.rs                     # CHANGED: process options, --match-tones, films, hidden --keep-capture
├── film.rs                    # NEW: FilmType, FilmStock, Profile, DustLevel, `color` rejection
├── gray.rs                    # NEW: GrayFrame accumulator from 8/16-bit RGB/RGBI chunks
├── tone.rs                    # NEW: histogram, auto points, blank check, tone lookup table
├── dust.rs                    # NEW: candidate search, flood fill, mask, repair
├── process.rs                 # NEW: per-frame pipeline (research P9), ProcessedFrame, tone matching
├── jpeg.rs                    # NEW: thin wrapper writing the 8-bit share copy through PartialFile
├── tiff.rs                    # CHANGED: 1-channel gray (BlackIsZero)
├── scan.rs                    # CHANGED: capture feeds GrayFrame; save writes master + JPEG + record v2
├── output.rs                  # CHANGED: free-stem check includes .jpg; publish record → jpg → tif
├── report.rs                  # CHANGED: progress/summary/JSON v2, films listing, resume with points
├── error.rs                   # unchanged (processing failures map to 9 / 10)
├── preset.rs, frame_stats.rs, capability.rs, usb.rs   # unchanged
└── backend/
    ├── mod.rs                 # CHANGED: ColorFormat::Rgb16
    ├── fake.rs                # CHANGED: negative-like frames with border and specks
    └── sane/mod.rs            # CHANGED: map Rgb16 to mode Color, depth 16
examples/
└── process_capture.rs         # NEW: replay a kept raw capture through the pipeline (tuning)
tests/
├── scan_cli.rs                # CHANGED: positives, processing options, usage errors, schema v2
├── sane_test_backend.rs       # CHANGED: real FFI path produces a positive
└── hardware.rs                # CHANGED: #[ignore] HP5/FP4 scan and short roll
README.md                      # CHANGED: scans are positives now; options; films
```

**Structure Decision**: same single crate. Processing is split into small modules, one per
idea, each with its own unit tests:
- `film`: what film it is;
- `gray`: building the frame;
- `tone`: tones;
- `dust`: dust;
- `process`: the order of the steps.

`scan.rs` keeps the sequencing and calls `process` between capture and save. Nothing in the
processing modules knows about SANE or the CLI.

## Implementation Order (for /speckit-tasks)

1. **Hardware first, briefly**: E9 (RGB vs RGBI timing) and E10 (linearity check) with
   `scanimage`. Neither blocks the code.
2. **Pure modules with tests**:
   - `film`;
   - `gray`;
   - `tone`, with the step wedge (SC-004), blank frame and border cases;
   - `dust`, with the specks and star field (SC-005);
   - `process`, with determinism and tone matching.
3. **Outputs**: 1-channel TIFF, the JPEG wrapper (new crate), output naming and publishing
   order, record v2.
4. **Wiring**: the capture feeds `GrayFrame`, then `save`, then CLI options and validation,
   `films`, report and JSON v2, then the fake backend's negative-like frames. Update the 002 CLI
   tests to the new outputs.
5. **Development tools**: `--keep-capture` and `examples/process_capture.rs`. Then E11:
   capture HP5 and FP4 fixtures and tune the profile and dust constants.
6. **Real-scanner validation** per quickstart.md: SC-001, SC-002, SC-003 and SC-006 recorded in
   `hardware-validation.md`. Update the README.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| A whole frame held in memory (36 MB at `standard`, up to ~105 MB with the dust mask at `high`), where the platform constraints prefer streaming "where the backend allows it" | Automatic black and white points need the whole frame's histogram before any output pixel is known. Dust repair needs neighbouring rows. Memory is still bounded per frame and independent of roll length. | Streaming the raw frame to a temporary file and making two passes would write 143–275 MB of raw data per frame that the owner chose not to keep, and would need crash cleanup. Keeping RGBI in memory would use four times the space for nothing. |
| Hidden `--keep-capture` flag and an example program, which no user story asks for | Tuning the profiles and dust thresholds (SC-001, SC-006, risk R1) needs the same real frame processed many ways. The constitution allows deliberately kept captures as fixtures. | Rescanning film for every tuning step means handling the film repeatedly (Principle II spirit) and takes 50–90 s per try. |
