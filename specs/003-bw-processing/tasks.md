---

description: "Task list for 003-bw-processing: Black-and-White Negative Processing"
---

# Tasks: Black-and-White Negative Processing

**Input**: Design documents from `/specs/003-bw-processing/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/cli.md](contracts/cli.md),
[contracts/scan-output.schema.json](contracts/scan-output.schema.json),
[quickstart.md](quickstart.md)

**Tests**: Included. Spec FR-022 and constitution Principle IV require every processing
behavior to be tested without the scanner. Hardware tests are `#[ignore]`d. Hardware steps are
recorded in `specs/003-bw-processing/hardware-validation.md`.

**Organization**: Tasks are grouped by user story, with labels following the spec's numbering:

| Label | Story | Priority |
|-------|-------|----------|
| US1 | Single frame to positive | P1 |
| US2 | Roll to positives | P1 |
| US3 | Dust removal | P2 |
| US4 | Tone adjustments | P2 |
| US5 | Matching tones across a roll | P3 |
| US6 | Positive previews | P3 |

US1 ships with dust removal wired as a pipeline step that is still a no-op; US3 fills it in
and makes `normal` the default.

**Implementation notes** (2026-11-09):

- **Dust is compared against the densest silver, not the white point.** T036 planned
  candidates as denser than the clipped white point + margin. The star-field test (T032) showed
  that on a night sky the stars cover less than the 0.05% white clip, so they were erased.
  Candidates are now denser than the densest non-opaque pixel (`tone::densest_silver`) +
  margin. The white point also ignores opaque pixels, via a new profile constant
  `opaque_range` (2.0 D), so heavy dust cannot set it. Recorded in research P5/P8 and the data
  model.
- **The fake film strip's specks are one pixel each, at raw 30**, not 3×3 at raw ≈ 300 (T014).
  Simulated frames are 1/50 of real size, where every dust size limit rounds to one pixel. The
  frame-number marker became `65535 − n` (bright film base), so it is never mistaken for dust.
- **`--black` / `--white` keep 3 decimals**, not 2 (T043), matching the recorded points, so a
  resumed `--match-tones` roll matches exactly.
- **Order of work**: the T035 dust constants and the T046/T047 tone lock were written together
  with the foundational and US1 code, since `scan.rs` was being rewritten anyway. Their tests
  came with their stories.
- **Not done (needs the scanner and the owner)**: T004, T005 (E9, E10), T052, T053 (E11 capture
  and tuning), T054 (optional fixture from E11), T056 (hardware validation). Until E9 runs, B&W
  scans capture RGBI and ignore the infrared samples (`BW_CAPTURE` in `src/scan.rs`).
- **Processing time**: a full `standard` frame takes 0.04 s in a release build
  (`processing_time_at_standard`, ignored by default), far inside SC-002's 18 s budget.

**Rules for every task** (constitution v2.0.0):

- **Rust concept comments**: the first time a concept from plan.md's "Rust Concepts Introduced"
  list appears, add a short comment explaining what it does and why it is used here.
- **`unwrap()`/`expect()`**: only in tests, or with a comment proving the call cannot fail.
- **No new `unsafe`**: processing code never touches FFI.
- **Done means**:
  - `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` all pass;
  - hardware-touching tasks are verified on the scanner and recorded (date, film stock, result)
    in `specs/003-bw-processing/hardware-validation.md`.
- **Film safety**: no task may run a command that moves film (`roll`, `advance`,
  `scanimage --advance=yes`) unless the owner is present and has loaded a test strip.
- **No raws**: outside the hidden `--keep-capture` flag, no code path may write raw capture
  data to disk (spec FR-014).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1–US6)

## Path Conventions

Single Cargo crate at the repository root: `src/`, `tests/`, `examples/`, `Cargo.toml`.

---

## Phase 1: Setup

**Purpose**: dependency, empty modules, hardware facts that shape the capture format.

- [X] T001 Add `jpeg-encoder = "0.6"` under `[dependencies]` in `Cargo.toml`, with a comment
  citing research P13 (pure-Rust grayscale JPEG encoder; JPEG coding is file-format plumbing,
  not core logic). Run `cargo build` to confirm it resolves.
- [X] T002 Add `pub mod film; pub mod gray; pub mod tone; pub mod dust; pub mod process; pub mod
  jpeg;` to `src/lib.rs`, and create each file with a `//!` module comment naming its research
  decision:
  - `src/film.rs`: P6;
  - `src/gray.rs`: P1, P2;
  - `src/tone.rs`: P4, P5;
  - `src/dust.rs`: P8;
  - `src/process.rs`: P9;
  - `src/jpeg.rs`: P10.

  `cargo build` must pass.
- [X] T003 Create `specs/003-bw-processing/hardware-validation.md` with the same table layout
  as `specs/002-film-scan/hardware-validation.md`. Include an "Experiments (E9–E11)" table and a
  "Quickstart validation" table with rows for quickstart steps 3–12 and SC-001, SC-002, SC-003,
  SC-006.
- [ ] T004 **Hardware, owner present.** Run experiment E9 from research.md and record it in
  `specs/003-bw-processing/hardware-validation.md`. With a B&W strip loaded and `--advance=no`,
  use the repo's SANE config (`SANE_CONFIG_DIR`, as in 002) to scan at 1800 dpi twice:
  `scanimage --mode Color --depth 16` and `scanimage --mode RGBI --depth 16`. Record:
  - both times;
  - the R/G/B channel means;
  - the mean absolute R/G/B difference between the two.

  Decision rule: if Color 16-bit is faster and the R/G/B difference is at noise level (≤ 1% of
  full scale), B&W uses `Rgb16`; otherwise it stays `Rgbi16`.
- [ ] T005 **Hardware, owner present.** Run experiment E10 and record it in
  `specs/003-bw-processing/hardware-validation.md`. At 1800 dpi, record the raw 16-bit means of:
  - the empty gate;
  - the clear film base (frame gap);
  - a dense highlight.

  Note whether the base/highlight ratio is plausible for `10^-D` with D ≈ 1–2. Write down any
  sign of a non-linear response (research P4 fallback: a fixed pre-curve).

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: film stocks, the gray frame, the tone maths and the new file writers, which every
story uses.

**⚠️ CRITICAL**: no user story work begins until this phase is complete.

- [X] T006 [P] Implement `src/film.rs` per data-model.md:
  - `enum FilmType { BwNegative }`, with `fn id(self) -> &'static str` returning
    `"bw-negative"` and a display name "B&W negative";
  - `#[derive(clap::ValueEnum)] enum FilmStock { Bw, Hp5, Fp4 }`, with `const DEFAULT: FilmStock
    = FilmStock::Bw`, `fn value()` (`"bw"`, `"hp5"`, `"fp4"`), `fn name()` ("Generic
    black-and-white negative", "Ilford HP5 Plus", "Ilford FP4 Plus") and `fn film_type()`;
  - `#[derive(clap::ValueEnum)] enum DustLevel { Off, Gentle, Normal, Strong }`;
  - `struct Profile { black_clip: f64, white_clip: f64, margin_mm: f64, min_range: f64,
    nominal_range: f64, brightness: i32, contrast: i32, dust: DustLevel }`;
  - `fn profile(self) -> Profile`, with one `match` arm per stock all returning the data-model
    start values (`black_clip` 0.001, `white_clip` 0.0005, `margin_mm` 2.0, `min_range` 0.15,
    `nominal_range` 1.2, brightness 0, contrast 0, dust `Off` for now, changed to `Normal` in
    T040), and a comment that each arm exists so a stock can be tuned alone (spec FR-012);
  - `fn parse_film(s: &str) -> Result<FilmStock, String>`: `"color"` (case-insensitive) returns
    `Err("Color film is not supported yet.")`, and anything else defers to `ValueEnum`.

  Unit tests: every stock's type is `BwNegative`, the default is `Bw`, `color` is rejected with
  that exact message, and an unknown value lists `bw, hp5, fp4`.
- [X] T007 [P] Implement `src/gray.rs`: `GrayFrame { width, height, dpi, pixels: Vec<u16>,
  filled }`.
  - `GrayFrame::new(params: &FrameParams, dpi: f64) -> Result<GrayFrame, ScanError>`
    pre-allocates `width × height` with checked multiplication (`usize::try_from`). A
    zero-sized frame is `ScanFailed`.
  - `push(&mut self, bytes: &[u8])` accepts 8- or 16-bit, 3- or 4-channel native-endian
    samples, as SANE delivers them (`u16::from_ne_bytes`). It averages R, G and B as
    `(r + g + b) / 3` in `u32`, scales 8-bit values by 257, and skips the 4th (infrared) sample.
    Samples and pixels split across chunks carry over to the next call (keep a small
    `Vec<u8>` of leftover bytes).
  - `is_complete()` returns true when `filled == width × height`.
  - Accessor `row(y) -> &[u16]`.

  Unit tests:
  - 8-bit and 16-bit, RGB and RGBI;
  - chunks split mid-sample and mid-pixel give the same result as one chunk;
  - infrared is ignored;
  - extra bytes beyond the frame are an error, not a panic.
- [X] T008 [P] Implement `src/tone.rs` part 1, the histogram and points (research P5):
  - `fn density(v: u16) -> f64 = log10(65535 / max(v,1))`.
  - `struct Histogram([u32; 65536])` boxed, or a `Vec<u32>` of length 65536, built from a
    `GrayFrame` over the picture area. The area excludes `margin_mm` on each side, converted
    with the frame dpi and clamped so at least the central half remains.
  - `struct TonePoints { black_density: f64, white_density: f64, source: PointSource }` with
    `enum PointSource { Auto, Explicit, Matched { frame: u32 }, BlankDefault }`.
  - `fn auto_points(hist: &Histogram, profile: &Profile) -> (TonePoints, bool /*probably
    blank*/)`:
    - `black_density` is the density at the `black_clip` fraction from the low-density end
      (brightest raw values);
    - `white_density` is the density at the `white_clip` fraction from the high-density end;
    - both are found with a running cumulative sum and rounded to 3 decimals;
    - if `white − black < min_range`, return `black` and `black + nominal_range` with source
      `BlankDefault`, and the blank flag true.

  Unit tests:
  - a uniform frame is probably blank;
  - a two-level frame gives exactly those two densities;
  - a 2 mm clear border does not change the points;
  - 0.01% of very dense pixels do not move the white point.
- [X] T009 [P] Implement `src/tone.rs` part 2, the lookup table (research P4):
  - `fn tone_table(points: &TonePoints, brightness: i32, contrast: i32) -> Vec<u16>`, 65,536
    entries.
  - For each raw value: `t = clamp((density(v) − black) / (white − black), 0, 1)`.
  - Contrast: `a = 2^(contrast/50)`, `t = tᵃ / (tᵃ + (1−t)ᵃ)`, guarding `t = 0` and `t = 1`.
  - Brightness: `g = 2^(−brightness/50)`, `t = t^g`.
  - Output: `round(t × 65535)`.
  - `fn apply(table: &[u16], frame: &mut GrayFrame)` maps every pixel in place.

  Unit tests:
  - the table is monotonically non-increasing in raw value (denser negative gives a lighter
    positive);
  - `black` maps to 0 and `white` to 65535;
  - with contrast 0 and brightness 0, the midpoint density maps to 32768 ± 1;
  - brightness +30 raises the mid value, contrast +30 lowers values at t = 0.25 and raises them
    at t = 0.75, and the endpoints are unchanged in both cases;
  - calling it twice gives identical tables (FR-007).
- [X] T010 [P] Extend `src/tiff.rs` to 1-channel gray. Accept `channels == 1` with
  `PhotometricInterpretation = 1` (BlackIsZero), no `ExtraSamples`, and `BitsPerSample` with a
  single value. Keep 3 and 4 channels unchanged and update the module comment. Unit tests: decode
  an 8-bit and a 16-bit gray file with the `tiff` dev-dependency and compare pixels.
- [X] T011 [P] Implement `src/jpeg.rs`:
  - `fn write_share_copy(dir: &Path, stem: &str, frame: &GrayFrame) -> Result<(PartialFile,
    File), ScanError>` creates `.<stem>.jpg.partial` through `output::PartialFile`.
  - It converts the 16-bit positive to 8-bit with `((v as u32 + 128) / 257) as u8`.
  - It encodes with `jpeg_encoder::Encoder::new(file, 90)` and `ColorType::Luma`.
  - It maps encoder errors to `ScanError::OutputError` with the target path.

  Unit test: a 64×32 gradient produces bytes that start with `FF D8`, end with `FF D9`, and
  whose SOF0 header gives 64×32 with 1 component.
- [X] T012 Update `src/output.rs`. `free_stem` treats a stem as taken if any of `.tif`, `.jpg`
  or `.json` exists. Replace `publish_scan` with `publish_frame(record, share: Option<…>,
  master)`, which publishes in the order record → share copy → master (research P10) and returns
  all paths. Keep `next_frame` counting `.tif` only. Unit tests: a lone `x-001.jpg` makes the
  next stem `x-001-2`, and publication order is visible by failing the master publish (the
  record and JPEG exist, no `.tif`).
- [X] T013 Add `ColorFormat::Rgb16` in `src/backend/mod.rs` and map it in
  `src/backend/sane/mod.rs` to mode `Color`, depth 16. Map it in `src/backend/fake.rs` to
  3-channel 16-bit frames. Add a fake-backend unit test that an `Rgb16` capture delivers
  `width × height × 6` bytes.
- [X] T014 Make the fake film strip negative-like in `src/backend/fake.rs`:
  - a clear-base border 2% of the frame wide on each side, at 80% of full scale;
  - a picture area whose density pattern varies with the frame number, spanning about 0.2–1.6 D
    (raw values `65535 × 10^−D`);
  - per-frame deterministic "grain" from a simple integer hash of (x, y, frame);
  - three opaque specks per frame (raw ≈ 300) of 3×3 pixels at fixed positions.

  Keep the first sample holding the frame number, and keep empty-gate and repeated-frame
  behavior unchanged. Update the existing fake-backend unit tests if needed.

**Checkpoint**: `cargo test` passes. Stocks, gray frames, tone maths, gray TIFF and JPEG writing
work on their own.

---

## Phase 3: User Story 1 – Scan a B&W frame straight to a positive (Priority: P1) 🎯 MVP

**Goal**: `primefilm scan --film hp5` saves `<roll>-NNN.tif` (16-bit gray positive),
`<roll>-NNN.jpg` and `<roll>-NNN.json` (record v2). No raw file is saved.

**Independent Test**: `primefilm --simulate ready scan --roll t --film hp5 --output /tmp/pf`
writes exactly `t-001.tif`, `t-001.jpg` and `t-001.json`. The TIFF decodes as 1-channel 16-bit,
the specks' raw-dark pixels are now light, the border is near black, and the JSON validates
against schema v2.

### Tests for User Story 1

- [X] T015 [P] [US1] Unit tests in `src/process.rs` for the step wedge (spec SC-004). Build a
  `GrayFrame` of 21 vertical bands from density 0.05 to 2.05 in 0.10 D steps, with a margin
  border at base density, and process it with stock `Bw`, dust off. Assert:
  - band means strictly increase with density (reversed order);
  - neighbouring bands differ by > 500 / 65535;
  - the lowest-density band is < 3% of full scale;
  - the highest-density band is > 97%.
- [X] T016 [P] [US1] Unit test in `src/process.rs`: processing the same frame twice with the same
  settings gives byte-identical pixels and identical `ProcessedFrame` (FR-007).
- [X] T017 [P] [US1] CLI tests in `tests/scan_cli.rs`, replacing the 002 assertions that expect
  RGBI raw output:
  - `scan` writes exactly `t-001.json`, `t-001.jpg` and `t-001.tif`, with no dotfiles;
  - the TIFF decodes as 1 sample per pixel, 16 bits;
  - the record has `record_version` 2, `processing.film_stock` `"hp5"`, `output.kind`
    `"positive-master"` and `points.source` `"auto"`;
  - `--json` validates against `specs/003-bw-processing/contracts/scan-output.schema.json`,
    with `channels` `"Gray"` and a `share_copy` path;
  - re-running `--frame 1` gives `t-001-2.*` and leaves the originals byte-identical;
  - `no-film`, `jam` and `disconnect` single scans leave no files and no `.partial`.

  Update the `schema()` helper to load the 003 schema.
- [X] T018 [P] [US1] CLI tests in `tests/scan_cli.rs` for film-stock usage errors:
  - `scan --film color` exits 2 with stderr containing "Color film is not supported yet." and
    writes no output folder;
  - `--film tri-x` exits 2 listing `bw, hp5, fp4`;
  - `films` prints the three stocks with `bw` marked default and the color line;
  - `films --json` validates against schema v2 with 3 entries.

### Implementation for User Story 1

- [X] T019 [US1] Implement `src/process.rs`:
  - `struct ProcessSettings { stock: FilmStock, brightness: i32, contrast: i32, points:
    Option<(f64, f64)>, dust: DustLevel }` with `fn from_profile(stock)`;
  - `struct ProcessedFrame { points: TonePoints, probably_blank: bool, dust_repaired: u32,
    seconds: f64 }`;
  - `fn process(frame: &mut GrayFrame, settings: &ProcessSettings, locked: Option<&TonePoints>)
    -> Result<ProcessedFrame, ScanError>`, running research P9:
    1. histogram → points: explicit if given (source `Explicit`), else `locked` (source as
       given), else `auto_points`;
    2. the dust step: call `dust::remove` when `settings.dust != Off`, a no-op stub returning 0
       until US3, then recompute auto points if they were automatic;
    3. `tone_table` + `apply`.

  An incomplete frame (`!is_complete()`) returns `ScanFailed`. Time the call.
- [X] T020 [US1] In `src/scan.rs`, change `capture`/`stream` for full scans: feed each chunk to
  `FrameStats` (unchanged) and to a `GrayFrame`, not a `TiffWriter`. Keep the existing raw
  `TiffWriter` path only behind `Option<KeepCapture>` (filled in T048). After a successful
  stream with the empty-gate and repeated-frame checks passing, call `process::process`. Full
  B&W scans use `ColorFormat::Rgb16` or `Rgbi16` per the T004 decision, held in one `const` with
  a comment citing E9.
- [X] T021 [US1] In `src/scan.rs`, rewrite `save` to write three files. The record v2
  (`ScanRecord` gains `output` and `processing` objects exactly as in data-model.md "Settings
  Record") is written via `PartialFile`. The master is `TiffWriter` with `channels: 1, bits: 16`,
  fed from `frame.pixels` in row chunks as native-endian bytes. The share copy is
  `jpeg::write_share_copy`. Publish with `output::publish_frame`. Bump `RECORD_VERSION` to 2.
  `SavedFile` gains `share_copy: Option<PathBuf>`, `film: FilmStock`, `probably_blank: bool` and
  `dust_repaired: u32`, and `channels` becomes `"Gray"`. A failure in any write drops all
  partials, so nothing is published.
- [X] T022 [US1] In `src/cli.rs`:
  - add `--film` (`value_parser = film::parse_film`, default `bw`) to `scan`, `roll` and
    `preview`;
  - add a `films` subcommand;
  - build `ProcessSettings::from_profile(stock)` in `src/lib.rs` and pass it into `RunOptions`
    (new field `process: ProcessSettings`);
  - dispatch `films`.

  Update the clap unit tests.
- [X] T023 [US1] In `src/report.rs`:
  - progress line `Frame NNN: saved <stem>.tif + .jpg (W × H, gray 16-bit) in N s; <film>`;
  - prefix `probably blank; ` when flagged;
  - run header shows the stock name;
  - JSON `schema_version` 2 with top-level `film` and the new per-file fields;
  - `print_films(json)` prints the table from contracts/cli.md, with "Color film is not
    supported yet." as the last text line.
- [X] T024 [US1] Move the 002 film-order check (the first sample of the saved TIFF) out of
  `tests/scan_cli.rs`. Replace it with a unit test in `src/scan.rs` asserting that each capture's
  `GrayFrame` first pixel encodes the fake frame number in order. Delete `first_sample` from the
  CLI tests.
- [X] T025 [US1] Update `tests/sane_test_backend.rs`: a scan through SANE's `test` backend
  produces a decodable 1-channel 16-bit `.tif`, a `.jpg` and a v2 `.json`. Adjust the mode set
  for `Rgb16` if T004 chose it.

**Checkpoint**: MVP. Single frames become positives with share copies. `cargo test` passes.

---

## Phase 4: User Story 2 – Scan a whole B&W roll straight to positives (Priority: P1)

**Goal**: `roll` produces a positive, JPEG and record per frame in film order. Resume, Ctrl-C,
jam and disconnect behave as in 002, and no raw or partial file is ever left.

**Independent Test**: `primefilm --simulate ready roll --roll r --film fp4 --output /tmp/pf`
gives 6 × 3 files `r-001…r-006` and `end-of-film`. The jam, disconnect and interrupt scenarios
keep finished frames and leave nothing else (SC-007).

### Tests for User Story 2

- [X] T026 [P] [US2] Update the roll CLI tests in `tests/scan_cli.rs`
  (`roll_scans_until_the_film_ends`, `roll_frame_limits`, `roll_ends_at_an_empty_gate`,
  `roll_jam_…`, `roll_disconnect`, `roll_interrupt_…`, `roll_with_no_film`):
  - expected file lists now hold `.json`, `.jpg` and `.tif` per frame;
  - after every failing scenario, `fs::read_dir` (including dotfiles) shows only those files
    (SC-007);
  - every record says `film_stock` `"fp4"`.
- [X] T027 [P] [US2] CLI test in `tests/scan_cli.rs`: the summary's resume command after a jam
  repeats `--film fp4`, and given `--dust off` it repeats that too.
- [X] T028 [P] [US2] Unit test in `src/scan.rs`: a processing failure after capture on frame 3
  (inject an incomplete `GrayFrame` through the fake `Jam`-style hook, or call `save` with an
  unwritable dir) stops the roll with `scan-failed`/`output-error` naming frame 003, and frames
  1–2 are kept.

### Implementation for User Story 2

- [X] T029 [US2] In `src/scan.rs` `roll_loop`, process and save each frame with
  `opts.process`. Map processing errors through `for_frame` so messages name the frame to
  rescan. Keep the order capture → end checks → process → save → log advance.
- [X] T030 [US2] In `src/report.rs`, the summary lists frames that are probably blank
  (`Probably blank: 001, 007.`). The resume command repeats every processing option the owner
  gave (`--film`, `--brightness`, `--contrast`, `--black/--white`, `--dust`), using `RunOptions`
  flags that record which ones were given, like 002's `quality_given`.

**Checkpoint**: whole rolls become positives. US1 and US2 together match the owner's normal
workflow.

---

## Phase 5: User Story 3 – Remove dust and specks (Priority: P2)

**Goal**: dust removal on by default (`normal`), with `--dust off|gentle|normal|strong`. Specks
are removed, stars kept, and nothing else is changed.

**Independent Test**: scan the same simulated frame with `--dust normal` and `--dust off`. The
images differ only within a few pixels of the fake's three specks, and the record shows
`repaired` 3.

### Tests for User Story 3

- [X] T031 [P] [US3] Unit test in `src/dust.rs` for SC-005, dust. Use a 1200×800 frame at
  1800 dpi with a mid-tone (1.0 D) and deterministic grain of ±0.05 D. Place 40 opaque specks
  (raw 200, 2–8 px across) and 10 hairs (1–2 px wide, 30–100 px long) at seeded positions.
  After `remove` at `normal`:
  - ≥ 95% of the 50 objects are repaired (no pixel in the object more than 0.1 D from the local
    mid-tone);
  - every pixel more than `dilate + 1` px from an object is bit-identical to the input.
- [X] T032 [P] [US3] Unit test in `src/dust.rs` for SC-005, stars. Place 100 Gaussian blobs
  (σ 1–2 px) peaking at the frame's `white_density` on a 0.2 D sky. After `remove` at `normal`,
  ≥ 95% of the blobs are unchanged.
- [X] T033 [P] [US3] Unit tests in `src/dust.rs`:
  - a dense component larger than `max_area` is left untouched (long scratch);
  - a speck touching the frame edge is repaired without panicking;
  - a speck covering a whole 64×64 window region falls back to the largest window (32 px)
    and still fills;
  - `gentle` repairs no more components than `normal`, and `normal` no more than `strong`;
  - the results are deterministic.
- [X] T034 [P] [US3] CLI tests in `tests/scan_cli.rs`:
  - the default scan records `dust.level` `"normal"` and `repaired` 3 (the fake's specks);
  - `--dust off` records `repaired` 0;
  - the two masters' pixels differ only within 5 px of the fake's speck positions;
  - `--dust bogus` exits 2.

### Implementation for User Story 3

- [X] T035 [US3] In `src/film.rs`, add
  `DustLevel::constants(self, dpi: f64) -> Option<DustConstants { margin: f64, max_area_px: u32,
  max_span_px: u32, dilate_px: u32 }>` from the research P8 table, verbatim:

  | Level | margin | max_area | max_span | dilate |
  |-------|--------|----------|----------|--------|
  | gentle | 0.50 | 0.05 mm² | 1.0 mm | 1 px |
  | normal | 0.30 | 0.10 mm² | 2.0 mm | 2 px |
  | strong | 0.15 | 0.25 mm² | 4.0 mm | 3 px |

  `off` returns `None`. Convert mm to px with `dpi / 25.4`, and mm² with its square.
- [X] T036 [US3] Implement candidate search and components in `src/dust.rs`.
  - The candidate threshold is a raw value `v_max = 65535 × 10^−(white_density + margin)`; a
    pixel is a candidate when `v ≤ v_max`. Do a single pass to collect candidate indices into a
    `Vec<bool>` mask sized `width × height`.
  - Group candidates into 8-connected components with an explicit stack (`while let Some(i) =
    stack.pop()`), tracking area and bounding box. Add a comment on why recursion is avoided.
  - Keep components with area ≤ `max_area_px` and both bounding-box sides ≤ `max_span_px`.
  - Return the kept components as lists of pixel indices.
- [X] T037 [US3] Implement the mask and repair in `src/dust.rs`.
  - Dilate the kept components by `dilate_px` into a repair mask.
  - Fill each masked pixel with the mean of the unmasked pixels in the smallest square window of
    radius 2, 4, 8, 16 or 32 that contains ≥ 8 of them; if none does, use radius 32 with
    whatever exists, or leave the pixel unchanged.
  - Read neighbours from an unmodified copy of the frame region (or process in two phases) so
    the result does not depend on fill order (FR-007).

  `pub fn remove(frame: &mut GrayFrame, white_density: f64, c: &DustConstants) -> u32` returns
  the number of components repaired.
- [X] T038 [US3] Wire dust into `src/process.rs`: replace the stub with `dust::remove`. When
  the points are `Auto`, recompute the histogram and auto points after repair. Fill
  `ProcessedFrame.dust_repaired`. In `src/scan.rs`, write the record's `processing.dust` object
  with `level`, `margin`, `max_area_px`, `max_span_px`, `dilate_px` and `repaired` (data-model
  record).
- [X] T039 [US3] Add `--dust off|gentle|normal|strong` to `scan` and `roll` in `src/cli.rs`
  (default from the profile). Show `dust <level>` in the roll header and `N dust specks
  removed;` in the progress line when N > 0 (`src/report.rs`).
- [X] T040 [US3] Change every stock's profile default dust level to `DustLevel::Normal` in
  `src/film.rs`, and update the T006 unit test.

**Checkpoint**: dust removal works by default and can be tuned or turned off.

---

## Phase 6: User Story 4 – Adjust the tones of a frame (Priority: P2)

**Goal**: `--brightness`, `--contrast` and `--black/--white` on `scan`, `roll` and `preview`,
validated before the scanner opens.

**Independent Test**: scanning a simulated frame with `--brightness 30` raises the master's
median while its minimum and maximum stay the same as the default scan. Out-of-range values
exit 2 without opening the scanner.

### Tests for User Story 4

- [X] T041 [P] [US4] CLI tests in `tests/scan_cli.rs`:
  - `--brightness 30` gives a higher master median than the default, the same 0.1th and
    99.9th percentiles ± 1%, and records `brightness: 30`;
  - `--contrast 30` widens the interquartile range and records `contrast: 30`;
  - `--black 0.30 --white 1.20` records `points.source` `"explicit"` with those values.
- [X] T042 [P] [US4] CLI usage-error tests in `tests/scan_cli.rs`, each exiting 2 with no output
  folder created:
  - `--brightness 51`;
  - `--contrast -51`;
  - `--black 0.2` alone;
  - `--white 1.0` alone;
  - `--black 1.2 --white 1.0`;
  - `--black 4.5 --white 5`;
  - `--black abc --white 1`.

### Implementation for User Story 4

- [X] T043 [US4] In `src/cli.rs`, add `--brightness` and `--contrast` with
  `value_parser!(i32).range(-50..=50)`, and `--black` / `--white` with a `parse_density`
  function returning `Result<f64, String>` (0.00–4.00, at most 2 decimals kept), using clap
  `requires` so each needs the other. Validate `black < white` in `src/lib.rs` before opening
  the backend, failing with a usage error (exit 2) whose message matches clap's style. Put the
  values into `ProcessSettings`.

**Checkpoint**: any frame can be rescanned lighter, darker, harder or softer, or with fixed
points.

---

## Phase 7: User Story 5 – Matching tones across a roll (Priority: P3)

**Goal**: `roll --match-tones` locks points from the first frame that is not probably blank.
The resume command carries the locked points.

**Independent Test**: `primefilm --simulate ready roll --roll m --match-tones` gives every
record the same `black_density`/`white_density`, with source `matched` from frame 1.
`--simulate interrupt` prints a resume command with `--black` and `--white`.

### Tests for User Story 5

- [X] T044 [P] [US5] Unit tests in `src/process.rs` for the data-model state machine:
  - Unlocked + probably blank frame → that frame uses `BlankDefault` and stays Unlocked;
  - Unlocked + normal frame → its auto points become `Locked(points, frame N)`;
  - Locked → later frames get `Matched { frame: N }` with identical densities;
  - explicit points ignore matching.
- [X] T045 [P] [US5] CLI tests in `tests/scan_cli.rs`:
  - `roll --match-tones` gives all records equal points with `source` `"matched"` and
    `matched_from_frame` 1, and JSON `tone_points` present (SC-008);
  - `--simulate interrupt roll --match-tones` exits 130 and its next step contains
    `--black <locked> --white <locked>` and not `--match-tones`;
  - `scan --match-tones` exits 2.

### Implementation for User Story 5

- [X] T046 [US5] In `src/process.rs`, add `struct ToneLock { state: Option<(TonePoints, u32)> }`
  with `fn points_for(&self) -> Option<TonePoints>` and `fn observe(&mut self, frame: u32,
  result: &ProcessedFrame)`, implementing the data-model transitions. In `src/scan.rs`
  `roll_loop`, create one lock when `--match-tones` is set, pass `locked` into `process`, and
  store the locked points in `RunReport` (new `tone_points: Option<(f64, f64, u32)>`).
- [X] T047 [US5] Add `--match-tones` to `roll` only in `src/cli.rs`. In `src/report.rs`, print
  `tones locked (black X D, white Y D)` on the locking frame. Add JSON `tone_points`. The
  resume command replaces `--match-tones` with `--black X --white Y` once locked, and keeps
  `--match-tones` if nothing was locked yet.

**Checkpoint**: a series can be scanned to match, including across an interrupted roll.

---

## Phase 8: User Story 6 – Positive previews (Priority: P3)

**Goal**: `preview` saves an 8-bit gray positive TIFF plus record in `previews/`, with no dust
removal, within the 15 s preview target.

**Independent Test**: `primefilm --simulate ready preview --film hp5 --output /tmp/pf` writes
`previews/preview-*.tif` (1 channel, 8 bits) and `.json` with `output.kind` `"preview"` and
`dust.level` `"off"`, and no `.jpg`.

### Tests for User Story 6

- [X] T048 [P] [US6] Update `preview_goes_to_its_own_folder` and `preview_with_no_film` in
  `tests/scan_cli.rs` to match the Independent Test above. `preview --brightness 20` records
  brightness 20, and `preview --dust normal` exits 2 (previews take no `--dust`).

### Implementation for User Story 6

- [X] T049 [US6] In `src/scan.rs` `preview_one`, capture `Rgb8` into a `GrayFrame` (scaled by
  257), process with dust forced off, and write the master as `TiffWriter { channels: 1, bits: 8
  }` from the 16-bit positive rounded to 8 bits (the same rounding as `jpeg.rs`; share the helper
  in `src/gray.rs`). Write record v2 with `output.kind` `"preview"`, and no share copy.

**Checkpoint**: all six stories work with the simulated scanner.

---

## Phase 9: Tuning tools and real-film tuning

**Purpose**: research P12 and E11. Tune the profile and dust constants on real HP5/FP4 captures
without rescanning.

- [X] T050 Add a hidden `--keep-capture DIR` global flag in `src/cli.rs`. In `src/scan.rs`, when
  it is set, full scans also stream the raw capture through the existing 002 `TiffWriter` path
  into `DIR/<stem>-capture.tif` (via `PartialFile`), and the record gains
  `processing.kept_capture`. Add a CLI test in `tests/scan_cli.rs` that the flag writes the
  capture only in `DIR`, never in the roll folder, and that without it nothing raw exists.
- [X] T051 [P] Create `examples/process_capture.rs`. It takes `CAPTURE.tif --film STOCK
  [--dust LEVEL] [--brightness N] [--contrast N] [--black D --white D] --out DIR`, decodes the
  RGB(I) TIFF with the `tiff` dev-dependency, pushes its bytes into `GrayFrame`, runs
  `process::process`, and writes `.tif`, `.jpg` and a printed summary (points, blank flag,
  repaired count, seconds). Uses only the library's public API, plus a small `std::env::args`
  parser (no clap needed). `cargo build --examples` must pass in clippy.
- [ ] T052 **Hardware, owner present.** Run experiment E11. With `--keep-capture
  ~/primefilm-captures`, scan at `web` 3–4 frames each of HP5 and FP4, including a dusty frame,
  a blank or leader frame and a high-key or night frame. Record the frames and conditions in
  `specs/003-bw-processing/hardware-validation.md`. Do not commit full captures.
- [ ] T053 Tune on the E11 captures with `examples/process_capture.rs`:
  - set the `black_clip`, `white_clip`, `min_range` and `nominal_range` start values per stock
    in `src/film.rs`;
  - set the dust margins and sizes in the T035 table;
  - record each changed constant with before/after evidence in
    `specs/003-bw-processing/hardware-validation.md`;
  - if E10 showed a non-linear response, add the fixed pre-curve to `tone_table` in
    `src/tone.rs` with a comment and a test;
  - update research.md P5/P8 and data-model.md with the final values.
- [ ] T054 [P] Optional fixture: crop one HP5 and one FP4 `web` capture (with dust) to ≤ 2 MB
  into `tests/fixtures/` and add a unit test in `src/process.rs` that processes it and checks
  that the result is not probably blank and repairs > 0 specks. Skip if no crop under 2 MB is
  representative, and note the decision in hardware-validation.md.

---

## Phase 10: Polish & Cross-Cutting Concerns

- [X] T055 [P] Update `tests/hardware.rs` with `#[ignore]` tests: `scan --film hp5 --preset web`
  gives 3 files and no raw, a 2-frame `roll --frames 2 --film fp4`, and a positive preview.
  These are run only with a strip loaded (`cargo test -- --ignored`).
- [ ] T056 **Hardware, owner present.** Run quickstart.md steps 3–12 on HP5 and FP4 strips and
  record them in `specs/003-bw-processing/hardware-validation.md`:
  - SC-001: count of frames needing no adjustment;
  - SC-002: time per frame vs 002's 91 s at `standard`, ≤ 109 s;
  - SC-003: folder size vs raw size, ≤ ⅓;
  - SC-006: frames needing no dust spotting;
  - SC-007: no leftover files;
  - the matched-roll resume.
- [X] T057 [P] Update `README.md`:
  - scans are now B&W positives (16-bit TIFF + JPEG + JSON), with no raw negatives kept;
  - the `--film`, `--brightness`, `--contrast`, `--black/--white`, `--dust` and
    `--match-tones` options;
  - the `films` command;
  - "color film is not supported yet";
  - extend the "reading order" list with the new modules.

  Remove the statement on line 15 that every scan is saved as a raw RGBI TIFF.
- [X] T058 [P] Review every new module (`src/film.rs`, `src/gray.rs`, `src/tone.rs`,
  `src/dust.rs`, `src/process.rs`, `src/jpeg.rs`, `examples/process_capture.rs`) against plan.md
  "Rust Concepts Introduced" items 1–10. Each concept has an explanatory comment at its first
  use, and there is no `unwrap`/`expect` outside tests without a justification comment.
- [X] T059 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test`, and fix anything outstanding. Then mark completed tasks `[X]` in this file.

---

## Phase 11: Revision after the first real scans (2026-11-09, research P15)

**Purpose**: the owner's first real HP5 frames showed outlines, flat patches and clipped
highlights. Tones move to post-processing, outputs are TIFFs only, and dust removal is off by
default.

- [X] T060 Remove all tone balancing: in `src/tone.rs`, replace the points, blank detection and
  tone table with `Inversion::{Linear, Density}` lookup tables; in `src/process.rs`, drop
  `ToneLock` and tone settings. Keep only the histogram measures dust removal needs
  (`base_density`, `densest_silver`).
- [X] T061 Remove `--brightness`, `--contrast`, `--black`, `--white`, `--match-tones` and
  `Cli::validate` from `src/cli.rs`, `src/lib.rs` and `src/main.rs`; trim `Profile` in
  `src/film.rs` to `black_clip`, `margin_mm`, `opaque_range` and `dust` (default `off`).
- [X] T062 Write each frame as `<stem>.tif` (linear master) and `<stem>-density.tif`, with the
  settings record embedded as JSON in `ImageDescription`; remove the `.json` sidecar, the JPEG
  share copy, `src/jpeg.rs` and the `jpeg-encoder` crate (`src/scan.rs`, `src/output.rs`,
  `Cargo.toml`). Publish the density file first and the master last.
- [X] T063 Update `src/report.rs` (no tone points or blank summary; resume repeats `--film` and
  `--dust`) and `examples/process_capture.rs` (writes both inversions).
- [X] T064 Update unit and integration tests (`tests/scan_cli.rs`, `tests/sane_test_backend.rs`,
  `tests/hardware.rs`), the JSON schema, the CLI contract, README, spec and design notes.
- [X] T065 ~~Choose an inversion on hardware~~: superseded by Phase 13 (both fixed inversions
  looked flat on a real roll preview).

## Phase 12: Roll preview (2026-11-09, spec User Story 7)

- [X] T066 [US7] Add `roll --preview` (`src/cli.rs`, `src/lib.rs`): conflicts with
  `--preset`, `--resolution` and `--dust`; dust forced off and not repeated when resuming.
- [X] T067 [US7] In `src/scan.rs`, `roll_settings` gives ordinary `Full` scans at
  `PREVIEW_DPI` (300) in 8-bit RGB with `advance_after` (the scanner's preview mode never moves
  film); roll previews go to `<output>/previews/<roll>/` and record `preset: preview`,
  `output.kind: preview`. The resume command adds `--preview` (`src/report.rs`).
- [X] T068 [P] [US7] Tests: `roll_preview_uses_ordinary_low_resolution_scans_that_advance`
  (`src/scan.rs`); roll preview files, record, resume and option conflicts
  (`tests/scan_cli.rs`). README, spec, contract updated.
- [ ] T069 [US7] **Hardware, owner present.** `primefilm roll --preview --roll test` on a
  4–6 frame strip: one preview per frame in film order, about 12 s per frame, the film
  advances and the run ends at the end of the strip. Record in `hardware-validation.md`.

## Phase 13: Automatic points restored (2026-11-09, research P16)

- [X] T070 Restore `TonePoints` (sources `auto`, `blank-default`), `auto_points` and a plain
  density `tone_table` in `src/tone.rs`; drop `Inversion`. Restore the profile's `white_clip`,
  `min_range`, `nominal_range` (`src/film.rs`) and the points step in `src/process.rs`; dust
  removal stays off by default.
- [X] T071 One TIFF per frame again (`src/scan.rs`): record gains `processing.points` and
  `processing.probably_blank`, loses `output.inversion`; `SavedFile` drops `density_image`,
  regains `probably_blank`; progress shows "probably blank". Example, tests, schema, contract,
  README and spec docs updated.
- [ ] T072 **Hardware, owner present.** Re-run `roll --preview` and a `web` roll on the HP5
  strip: no outlines, flat patches or flat grey frames (SC-001). Record in
  `hardware-validation.md`.

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: T001–T003 have no dependencies. T004/T005 need the owner and a strip and
  can happen any time before T020. T020 needs T004's decision; until then, default to `Rgbi16`.
- **Foundational (Phase 2)**: needs T001–T002. Blocks every story.
- **US1 (Phase 3)**: needs Phase 2. It is the MVP.
- **US2 (Phase 4)**: needs US1, since it reuses its capture, process and save path.
- **US3 (Phase 5)**: needs US1 (the pipeline stub in T019). It is independent of US2, US4, US5
  and US6.
- **US4 (Phase 6)**: needs US1 (the process settings). It is independent of US3.
- **US5 (Phase 7)**: needs US2 (the roll loop) and US4's `--black/--white` parsing for the
  resume command.
- **US6 (Phase 8)**: needs US1. It is independent of the others.
- **Tuning (Phase 9)**: needs US1 and US3. T052 needs the owner.
- **Polish (Phase 10)**: needs everything above.

### Story completion order

```text
Setup → Foundational → US1 ─┬─ US2 ── US5
                            ├─ US3 ──┐
                            ├─ US4 ──┼── Tuning → Polish
                            └─ US6 ──┘
```

### Within each story

Tests first: write them, see them fail, then implement. Then the processing modules, then the
`scan.rs` wiring, then the CLI and report.

---

## Parallel Execution Examples

**Phase 2**: T006, T007, T008, T009, T010 and T011 touch different files and can run together.
T008 and T009 share `src/tone.rs`, so they run in sequence when done by one person. Then T012,
T013 and T014.

**US1**: T015, T016, T017 and T018 together. Then T019 → T020 → T021 in sequence (shared
`scan.rs`/`process.rs`), with T022 and T023 alongside them once T021's types exist.

**US3**: T031, T032, T033 and T034 together. Then T035 → T036 → T037 → T038, with T039 alongside.

**After US1**: US3, US4 and US6 can proceed in parallel, mostly in separate files. They do share
`src/cli.rs` and `tests/scan_cli.rs`, so merge with care.

---

## Implementation Strategy

### MVP first (US1 only)

Phases 1–3: a single B&W frame becomes a positive TIFF + JPEG + record with automatic tones.
Validate with the simulated scanner, then with one real HP5 frame (quickstart step 4).

### Incremental delivery

1. US1, then US2: the owner's normal workflow (whole rolls to positives).
2. US3: dust removal on by default.
3. US4: fixing individual frames.
4. US5 and US6: series matching and positive previews.
5. Tuning on real captures (Phase 9), then the hardware validation and README (Phase 10).

Each step leaves `cargo test` green and the tool usable.
