---

description: "Task list for 002-film-scan: Film Scanning (Single Frame, Roll, Preview)"
---

# Tasks: Film Scanning (Single Frame, Roll, Preview)

**Input**: Design documents from `/specs/002-film-scan/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/cli.md](contracts/cli.md),
[contracts/scan-output.schema.json](contracts/scan-output.schema.json),
[quickstart.md](quickstart.md)

**Tests**: Included. FR-021 and constitution Principle IV require every non-hardware behavior to
be tested without the scanner. Hardware tests are `#[ignore]`d and hardware steps are recorded
in [hardware-validation.md](hardware-validation.md).

**Organization**: Tasks are grouped by user story. Story labels keep the spec's numbering
(US1 = roll, US2 = single frame, US3 = preview, US4 = presets, US5 = advance). The single-frame
story (US2) is built before the roll story (US1): both are P1, roll scanning repeats the
single-frame capture, and single frames don't depend on the film-transport experiments.

**Implementation notes**: after hardware experiment E8 the owner dropped the `max` (10000 dpi)
preset: the scanner resolves at most 5000 dpi across the frame. Tasks below that mention
`max` were implemented with web/standard/high and `--resolution` 25–5000.
 the scanning commands' CLI tests went into a new file,
`tests/scan_cli.rs`, rather than `tests/cli.rs`, to keep feature 001's tests separate.

**Rules for every task** (constitution v1.1.0, as in feature 001):

- **Rust concept comments**: the first time a concept from plan.md's "Rust Concepts Introduced"
  list appears, add a short comment explaining what it does and why it's used here.
- **`unwrap()`/`expect()`**: only in tests, or with a comment proving the call cannot fail.
- **`unsafe`**: only under `src/backend/sane/`, each block with a `// SAFETY:` comment.
- **Done means**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` all pass. Hardware-touching tasks are also verified on the scanner and recorded
  (date, film type, result) in `specs/002-film-scan/hardware-validation.md`.
- **Film safety**: no task may run a command that moves film (`roll`, `advance`,
  `scanimage --advance=yes`) unless the owner is present and has loaded a test strip.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1–US5)

## Path Conventions

Single Cargo crate at the repository root: `src/`, `tests/`, `Cargo.toml`, `sane.d/`.

---

## Phase 1: Setup

**Purpose**: dependencies and empty modules.

- [X] T001 Update `Cargo.toml`: add `jiff = "0.2"` under `[dependencies]` and a
  `[dev-dependencies]` section with `tiff = "0.11"` (test-only decoder, research D11). Run
  `cargo build` to confirm both resolve.
- [X] T002 Add `pub mod preset; pub mod scan; pub mod output; pub mod tiff; pub mod frame_stats;`
  to `src/lib.rs` and create the five files (`src/preset.rs`, `src/scan.rs`, `src/output.rs`,
  `src/tiff.rs`, `src/frame_stats.rs`), each with a `//!` module doc line naming its job from
  plan.md's source tree.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: learn how the XA moves film, then build the pieces every scanning command uses:
error outcomes, presets, the TIFF writer, frame statistics, safe file output, the scan session
interface with its SANE and fake implementations, frame capture, and reporting.

**⚠️ CRITICAL**: no user story work can begin until this phase is complete. The hardware spike
(T003–T007) needs the owner at the scanner with a 4–6 frame test strip.

### Hardware spike (research.md E1–E7)

- [X] T003 Create `scripts/spike-sane-config.sh FLAGS`: copies the repo's `sane.d/dll.conf`
  and `sane.d/pieusb.conf` into a new temp directory, replaces the last field of the
  `usb 0x05e3 0x0144 0x0048 …` line with `FLAGS` (e.g. `0x01`), and prints
  `export SANE_CONFIG_DIR=<dir>` for the owner to paste. Read-only towards the scanner.
- [X] T004 With the owner: run E1 from research.md (flags `0x01`, then `0x03` if `sane_start`
  fails on command 0x17 in the `SANE_DEBUG_PIEUSB=5` log), using
  `scanimage --mode Color --depth 8 --resolution 300 --advance=no -o e1.tiff`. Record the
  result in `specs/002-film-scan/hardware-validation.md`, then set the working flags value in
  `sane.d/pieusb.conf` with a comment explaining it (research D2). **If neither value works,
  stop: US1 and US5 are blocked; continue with US2–US4 and tell the owner a direct-USB backend
  feature is needed.**
- [X] T005 With the owner: run E2 (two scans with `--advance=no`; does the film move? compare
  images) and E3 (three scans with `--advance=yes`; exactly one frame each). Record in
  `hardware-validation.md`. **If E2 shows film moving on a non-advancing scan, stop and report
  to the owner before any further scanning work** (FR-014, Principle II).
- [X] T006 With the owner: run E4 (scan with `--advance=yes` past the last frame of the strip)
  and E5 (scan with no film loaded). For each, record the `scanimage` exit status, the status
  and sense codes from the `SANE_DEBUG_PIEUSB=5` log, and keep the images (outside the repo)
  for T011's thresholds. Record in `hardware-validation.md` and add a "Findings from the
  hardware" section to `specs/002-film-scan/research.md` stating exactly which status means
  *end of film* and *no film* (D4 layer 1), or that none is usable.
- [X] T007 With the owner: run E6 (`--preview=yes`: image size and seconds) and E7 (1800, 3600,
  5000 dpi at Color 8-bit: compare pixel sizes with data-model.md's preset table). Record in
  `hardware-validation.md` and research.md.

### Shared building blocks

- [X] T008 [P] Extend `ScanError` in `src/error.rs` with `NoFilm` (exit 8, outcome `no-film`,
  "No film found in the scanner.", next step "Insert a film strip and wait for the scanner to
  load it."), `ScanFailed { frame: Option<u32>, text: String }` (exit 9, `scan-failed`, "The
  scanner reported an error while scanning frame NNN: <text>.", next step "Check the film for
  a jam (use the eject button if needed), then resume."), and `OutputError { path: String,
  reason: String }` (exit 10, `output-error`, "Could not write <path>: <reason>.", next step
  "Check free space and permissions of the output folder."). Make `Disconnected`'s `during`
  read "scanning frame NNN" when a frame is known. Update the unit tests: all 10 exit codes
  distinct, none 0 or 2.
- [X] T009 [P] Implement `src/preset.rs`: `enum Preset { Web, Standard, High, Max }` with
  `dpi()` (1800, 3600, 5000, 10000), `Preset::DEFAULT = Standard`, `name()`, and `impl Display`;
  constants `SCAN_AREA_MM = (36.499786376953125, 24.297637939453125)` (feature 001 recording);
  `frame_pixels(dpi) -> (u32, u32)` computing `floor(mm / 25.4 × dpi)`; `rgbi16_bytes(dpi)`
  = width × height × 8; `seconds_per_frame() -> Option<f64>` returning `None` until T051 fills in measured times.
  Unit tests assert data-model.md's table exactly: web 2586 × 1721, standard 5173 × 3443,
  high 7184 × 4782, max 14369 × 9565.
- [X] T010 [P] Implement the streaming TIFF writer in `src/tiff.rs` (research D3):
  `TiffLayout { width, height, channels: 3 | 4, bits: 8 | 16, dpi, description, software,
  datetime }`; `TiffWriter::new(file: File, layout) -> io::Result<TiffWriter>` writes the
  8-byte little-endian header (`II`, 42, IFD offset placeholder); `write(&mut self, bytes:
  &[u8])` appends sample data, converting 16-bit samples from native to little-endian (keep any
  odd trailing byte for the next call); `finish(self) -> io::Result<File>` fails unless exactly
  `width × height × channels × bits/8` bytes were written, then writes the IFD (tags in
  ascending order: ImageWidth, ImageLength, BitsPerSample, Compression=1,
  PhotometricInterpretation=2, ImageDescription, StripOffsets, SamplesPerPixel, RowsPerStrip,
  StripByteCounts, XResolution, YResolution, PlanarConfiguration=1, ResolutionUnit=2 (inch),
  Software, DateTime, and ExtraSamples=[0] only when channels = 4) and patches the header.
  Strips: as many whole rows as fit in 64 KiB (at least 1). Memory stays bounded by a
  `BufWriter`. Unit tests decode the output with the `tiff` dev-dependency for RGB 8-bit and
  RGBI 16-bit images: dimensions, pixel values, resolution tags, ExtraSamples = 0; plus a test
  that `finish` rejects a short image.
- [X] T011 [P] Implement `src/frame_stats.rs` (data-model "Frame Statistics", research D4):
  `FrameStats::new(width, height, channels, bits)`, `update(&mut self, bytes: &[u8])` that
  handles samples split across chunk boundaries and tracks the pixel position; per color
  channel (R, G, B; ignore I) the fraction of samples ≥ `NEAR_FULL` (0.5 of full scale) and
  `looks_empty()` = every color channel's fraction ≥ `EMPTY_FRACTION` (0.95); a `Fingerprint`
  of 16 × 16 cells × 3 color channels holding each cell's mean as a fraction of full scale,
  with `same_frame(&a, &b)` = mean absolute difference < `SAME_FRAME_DIFF` (0.01). Comment
  each constant with the E4/E5 measurement it comes from. Unit tests: a light-source frame
  (80% of full scale, as measured in E5) is empty; a black frame, an orange-base-colored
  frame and a gradient are not; a frame split into odd-sized chunks gives the same result as
  one chunk; a frame plus ±1% noise is the same frame as itself; two different gradients are
  not.
- [X] T012 [P] Implement `src/output.rs` (research D7, D8; data-model "Roll", "Saved Scan",
  "Preview"): `default_output_root()` = `$HOME/Pictures/primefilm` (`OutputError` if `HOME` is
  unset); `validate_roll_name` enforcing "1–64 characters from `A–Z a–z 0–9 - _ .`, not
  starting with `.`"; `default_roll_name()` = today's local date `YYYY-MM-DD` via `jiff`;
  `RollFolder::open(root, name)` creating `<root>/<name>/`; `next_frame()` = "1 + the highest
  `NNN` among files named `<name>-NNN*.tif` in `folder`; 1 if none"; `scan_stem(frame)` =
  `<roll>-<NNN>` (frame 1–999, 3 digits) with `-2`, `-3`, … appended while a `.tif` or `.json`
  with that stem exists; `preview_stem()` = `previews/preview-<YYYYMMDD-HHMMSS>` (same suffix
  rule); `PartialFile::create(dir, final_name)` writing `.<final_name>.partial`;
  `PartialFile::publish(self)` = `sync_all`, `std::fs::hard_link(partial, final)` (fails if
  `final` exists, never replaces), remove partial; dropping an unpublished `PartialFile`
  deletes it. `publish_scan(sidecar, tiff)` publishes the sidecar first, then the TIFF. With
  `-v`, list any leftover `.partial` files found in the folder. All I/O errors become
  `ScanError::OutputError { path, reason }`. Unit tests in temp directories: next-frame rules,
  suffixing, an existing file is never modified (compare bytes) even when `publish` targets
  its name, a dropped partial leaves no file, invalid roll names rejected.
- [X] T013 Extend `src/backend/mod.rs` (research D1, data-model "Scan Settings", "Frame
  Layout"): `ScanKind { Full, Preview, Throwaway }`, `ColorFormat { Rgbi16, Rgb8 }`,
  `ScanSettings { kind, resolution_dpi: f64, color, advance_after: bool }`, `FrameParams {
  pixels_per_line, lines, bits_per_sample, channels, bytes_per_line }` with a
  `FrameParams::from_sane(bytes_per_line, pixels_per_line, lines, depth)` constructor
  enforcing: pixels and lines > 0; bits 8 or 16; `channels = bytes_per_line / (pixels_per_line
  × bits/8)` must be 3 or 4; `bytes_per_line` must equal `pixels × channels × bytes` exactly
  (otherwise `ScanFailed`). Trait
  `ScanSession { fn device(&self) -> &DeviceInfo; fn configure(&mut self, &ScanSettings) ->
  Result<(), ScanError>; fn start_frame(&mut self, cancel: &AtomicBool) -> Result<FrameParams,
  ScanError>; fn read(&mut self, buf: &mut [u8]) -> Result<usize, ScanError> /* 0 = end of
  frame */; fn end_frame(&mut self); fn options(&mut self) -> Result<Vec<Capability>,
  ScanError>; }` and on `ScannerBackend` add `type Session: ScanSession;` and
  `fn open(&mut self, device: &str) -> Result<Self::Session, ScanError>`, with the
  associated-type concept comment (plan concept 2). Unit tests for `FrameParams::from_sane`
  including pieusb's RGBI case (format RGB, `bytes_per_line = 2 × 4 × pixels`).
- [X] T014 [P] Extend `src/backend/sane/ffi.rs`: `#[repr(C)] SaneParameters { format,
  last_frame, bytes_per_line, pixels_per_line, lines, depth }` (all `i32`; size test = 24),
  `ACTION_SET_VALUE = 1`, `INFO_INEXACT = 1`, `INFO_RELOAD_OPTIONS = 2`, `INFO_RELOAD_PARAMS =
  4`, and `sane_start`, `sane_get_parameters`, `sane_read(handle, buf: *mut u8, max_len: i32,
  len: *mut i32)`, `sane_cancel`. Update the module doc: this feature scans and sets options.
- [X] T015 Change ownership in `src/backend/sane/mod.rs` (research D1): `Sane::open(self,
  name) -> Result<Device, ScanError>` consumes the context; `Device { handle, sane: Sane }`
  with the handle field first and a comment that fields drop in declaration order, so
  `sane_close` runs before `sane_exit` (plan concept 1). Remove the `Device<'a>` lifetime and
  adapt `SaneBackend::read_capabilities`. Feature 001 tests (`tests/cli.rs`,
  `tests/sane_test_backend.rs`) must still pass unchanged.
- [X] T016 Implement `SaneSession` in `src/backend/sane/mod.rs`: `Device::set_option(name,
  value)` finding the option by name and writing Bool / Int / Fixed (`(v × 65536.0).round() as
  i32`, plan concept 9) / String with `ACTION_SET_VALUE`, logging the value read back when SANE
  reports `INFO_INEXACT`. `configure` applies data-model.md's pieusb mapping exactly — Full:
  `mode=RGBI`, `depth=16`, `resolution=<dpi>`, `preview=false`, `advance=<advance_after>`,
  `clean-image=false`, `correct-infrared=false`; Preview: `mode=Color`, `depth=8`,
  `preview=true`, `advance=false`; Throwaway: `mode=Color`, `depth=8`, `resolution=25`,
  `advance=true` — and, when the backend is not `pieusb`, a test-backend profile (`mode=Color`,
  `depth` 8 or 16, `resolution`; `Rgbi16` falls back to 16-bit RGB) for testing. Every option
  value sent is logged with `-v`. `start_frame`: `sane_start`; a failure goes through
  `SaneBackend::explain` (disconnect → `Disconnected`, busy, access denied) and any other
  failure becomes `ScanFailed` (the XA has no end-of-film or no-film status, T006); then
  `sane_get_parameters` → `FrameParams::from_sane`. `read`: `sane_read` until
  `SANE_STATUS_EOF` (return 0). `end_frame`: `sane_cancel`. `options`: reuse the 001 option
  reader. `SaneBackend::open` runs the 001 diagnosis, `Sane::init`, device listing, and
  `Sane::open`.
- [X] T017 [P] Add the simulated film strip in `src/backend/fake.rs` (research D12):
  `FakeSession` with a film `position` (starts at 1) and `frames` (default 6). Like the XA
  (E4/E5), advancing past the last frame leaves the film on the last frame, and the `NoFilm`
  scenario returns bare-light-source frames (80% of full scale); frame size is
  preset size at `dpi / 100` (e.g. standard → 51 × 34) so tests stay fast; data is a gradient
  in RGBI 16-bit or RGB 8-bit whose first sample encodes the frame number, so tests can tell
  frames apart; `end_frame` moves `position` forward by one if the configured settings had
  `advance_after`. Scenarios: `Jam` (frame 3 → `ScanFailed`), `Disconnect` (frame 3 →
  `Disconnected { during: "scanning frame 3" }`), `Interrupt` (sets `cancel` during frame 2),
  `EmptyGate` (frame 4 is a bare-light-source frame), `NoFilm`. Expose `position()` for unit
  tests. Existing 001 scenario behavior for `detect`/`capabilities` is unchanged.
- [X] T018 [P] Add the hidden test flags to `src/cli.rs`: `Scenario` gains `NoFilm`, `Jam`,
  `EmptyGate`; global hidden `--simulate-frames <N>` (default 6). Pass both to `FakeBackend`
  in `src/lib.rs`. Unit test parsing.
- [X] T019 Implement frame capture in `src/scan.rs`: `capture_frame(session, settings,
  partial: PartialFile, layout_info, log, cancel) -> Result<CapturedFrame, ScanError>` that
  calls `start_frame`, builds a `TiffLayout` from `FrameParams` (description names roll,
  frame and preset; software `primefilm <version>`; datetime via `jiff`), reads into one reused
  1 MiB buffer (plan concept 7) feeding both `TiffWriter` and `FrameStats`, calls `end_frame`
  on every path, and returns params, stats (with fingerprint), seconds and the unpublished
  file. On any error the
  partial is dropped (deleted). Also `ScanRecord` (serde) with research D10's fields: schema
  version, tool version, local timestamp with UTC offset, scanner identity, roll, frame,
  preset, requested and actual resolution, color mode, bit depth, channel order `R,G,B,I`,
  pixel size, seconds, film movement, and the full option snapshot from `session.options()`.
  Unit tests with `FakeSession`: a captured frame decodes with the `tiff` crate and
  `end_frame` is called after errors.
- [X] T020 [P] Add scan reporting to `src/report.rs` per contracts/cli.md: progress lines on
  stderr (`Frame NNN: scanning…`, `Frame NNN: saved <file> (W × H, RGBI 16-bit) in N s; film
  advanced|film not moved`), the stdout summary (`Saved N frames (AAA–BBB) to <folder> in …`
  plus the resume command when stopped early), and the `--json` `ScanReport` struct matching
  `contracts/scan-output.schema.json` (`schema_version`, `outcome`, optional `message` and
  `next_step`, `roll`, `folder`, `preset`, `resolution_dpi`, `files[]` with `frame`, `image`,
  `settings`, `width`, `height`, `channels`, `bits`, `seconds`, `film_moved`). Unit tests for
  the resume command text and JSON field names.
- [X] T021 [P] Update the Ctrl-C handler in `src/main.rs` (research D6): on the first Ctrl-C
  print "Stopping after this frame is saved (press Ctrl-C again to abort now)." to stderr and
  set the flag; the second Ctrl-C still exits 130 immediately.

**Checkpoint**: `cargo test` passes, including all feature 001 tests; `pieusb.conf` flags are
set from hardware evidence (or the transport block is recorded).

---

## Phase 3: User Story 2 - Scan a single frame (Priority: P1) 🎯 MVP

**Goal**: `primefilm scan` saves exactly one archival frame and leaves the film where it was.

**Independent Test**: load a strip, run `primefilm scan` twice; two files (`-001`, `-002`) of
the same frame, film not moved; an existing file is never replaced.

### Tests for User Story 2

- [X] T022 [P] [US2] CLI tests in `tests/cli.rs` (temp output dirs): `--simulate ready scan
  --roll t --output D` → exit 0, `t/t-001.tif` + `t-001.json`, stdout names the file, no
  "film advanced" on stderr; again → `t-002.tif`; `--frame 1` → `t-001-2.tif` while
  `t-001.tif` bytes are unchanged; `--simulate no-film scan` → exit 8, no file; `--simulate busy
  scan` → exit 4 with the 001 failure JSON; `--json` output parses and has the schema's
  required fields; no `.partial` files remain after any run; invalid `--roll "a/b"` → exit 2.
- [X] T023 [P] [US2] Unit tests in `src/scan.rs` with `FakeSession` positioned at frame 3:
  `Jam` → `ScanFailed` and no files; `Disconnect` → `Disconnected`; film `position` unchanged
  after a successful single scan; an empty-gate frame → `NoFilm`.
- [X] T024 [P] [US2] Real-FFI test in `tests/sane_test_backend.rs`: `--sane-backend test scan
  --roll t --output D` → exit 0; decode the TIFF with the `tiff` crate and check its size
  matches the sidecar's `width`/`height`.
- [X] T025 [P] [US2] `#[ignore]` hardware test in `tests/hardware.rs`: `scan --roll hwtest
  --output <temp>` → exit 0, one TIFF with 4 channels at 16 bits.

### Implementation for User Story 2

- [X] T026 [US2] Add the `scan` subcommand to `src/cli.rs`: `--roll NAME` (validated by
  `output::validate_roll_name`), `--frame N` (1–999), `--output DIR`, `--device NAME`.
- [X] T027 [US2] Implement `scan_single` in `src/scan.rs`: resolve roll (default date), folder,
  frame (`--frame` or `next_frame()`), stem; `backend.open`; `configure` Full at the default
  preset's dpi with `advance_after = false`; `capture_frame`; `looks_empty()` → `NoFilm` (file
  discarded); write the `ScanRecord` sidecar and `publish_scan`. Returns a
  run result with one file, `film_moved: none`.
- [X] T028 [US2] Dispatch `Command::Scan` in `src/lib.rs` and print via `report` (progress,
  path, JSON).
- [X] T029 [US2] With the owner: quickstart "With the scanner" step 3 and `cargo test --
  --ignored` for T025; record in `hardware-validation.md`.

**Checkpoint**: single-frame scanning works on the fake backend, SANE `test` and the XA.

---

## Phase 4: User Story 1 - Scan a whole strip or roll unattended (Priority: P1)

**Goal**: `primefilm roll` scans every frame until the film ends, saving one numbered file per
frame, stopping safely and resumably on Ctrl-C, disconnects and errors.

**Independent Test**: a 4–6 frame strip yields one file per frame in film order, no skips or
duplicates, `end-of-film`, exit 0.

**Requires**: T004–T006 succeeded (film transport works); otherwise skip to Phase 5.

### Tests for User Story 1

- [X] T030 [P] [US1] CLI tests in `tests/cli.rs`, following quickstart.md's table: `--simulate
  ready roll --roll r` → exit 0, outcome `end-of-film`, `r-001`…`r-006`, each frame's first
  sample matching its number; `--simulate-frames 3` → 3 files; `--frames 2` → 2 files, exit 0;
  `--simulate empty-gate` → 3 files, exit 0; the default strip ends by the same-frame check
  (frame 7 repeats frame 6 and is not saved); `--simulate jam` → exit 9, 2 files, next step
  `primefilm roll --roll j --start-frame 3`, no `.partial` left; `--simulate disconnect` →
  exit 7, 2 files; `--simulate interrupt` → exit 130 with frame 2 saved; running `roll` again
  on a folder holding 001–002 starts at 003; `--simulate no-film roll` → exit 8; `--json` has
  `files` with 6 entries and every `film_moved` = `advanced`.
- [X] T031 [P] [US1] Unit tests in `src/scan.rs`: the roll loop advances the fake film exactly
  once per saved frame; with `--start-frame 1` on a folder holding `r-001.tif`, the new frame
  is `r-001-2.tif` and the old file is unchanged.
- [X] T032 [P] [US1] `#[ignore]` hardware test in `tests/hardware.rs`: `roll --preset web
  --frames 2` on a loaded strip → exit 0, 2 files.

### Implementation for User Story 1

- [X] T033 [US1] Add the `roll` subcommand to `src/cli.rs`: `--roll`, `--start-frame N`
  (1–999), `--frames N` (≥ 1; `--frames 0` is a usage error), `--output`, `--device`.
- [X] T034 [US1] Implement `scan_roll` in `src/scan.rs` following data-model.md's roll state
  diagram: `configure` once with `advance_after = true`; for each frame print progress,
  `capture_frame`; on the first frame an empty gate → `NoFilm`; on later frames an empty
  gate or a fingerprint matching the previous frame's → stop with `end-of-film` (frame
  discarded); otherwise write sidecar, `publish_scan`, log
  `film advanced` (FR-013); stop at `--frames`; after saving, stop with `Interrupted` if
  `cancel` is set; on any error stop at once (no retry, FR-017) keeping saved files. The run
  result always carries the list of saved files and the next frame number for the resume
  hint.
- [X] T035 [US1] Dispatch `Command::Roll` in `src/lib.rs`; print the summary and, for
  `interrupted`, `disconnected`, `scan-failed` and `output-error`, the resume command
  (`primefilm roll --roll R --start-frame N`) as the next step; for `disconnected` add "check
  where the film is" (contracts/cli.md).
- [ ] T036 [US1] With the owner: quickstart steps 5 and 6 (full strip; Ctrl-C during frame 2,
  then resume) and `cargo test -- --ignored` for T032; record in `hardware-validation.md`,
  including any disconnects observed (research R2).

**Checkpoint**: roll scanning works end to end; interrupted rolls resume with continuous
numbering.

---

## Phase 5: User Story 3 - Quick low-resolution preview (Priority: P2)

**Goal**: `primefilm preview` saves a quick, viewable, non-archival image of the frame in the
scanner without moving the film.

**Independent Test**: preview completes in under 15 s, opens in an image viewer, and the film
does not move.

### Tests for User Story 3

- [X] T037 [P] [US3] CLI tests in `tests/cli.rs`: `--simulate ready preview --output D` →
  exit 0, `D/previews/preview-*.tif` (8-bit RGB, 3 channels) plus its `.json`, nothing written
  in any roll folder, no "film advanced"; `--simulate no-film preview` → exit 8.
- [X] T038 [P] [US3] `#[ignore]` hardware test in `tests/hardware.rs`: `preview` → exit 0
  within 15 s.

### Implementation for User Story 3

- [X] T039 [US3] Add the `preview` subcommand to `src/cli.rs` (`--output`, `--device`).
- [X] T040 [US3] Implement `preview` in `src/scan.rs`: settings `Preview`, `Rgb8`,
  `advance_after = false`; path from `output::preview_stem()`; empty gate → `NoFilm`;
  sidecar without roll and frame; dispatch in `src/lib.rs`.
- [X] T041 [US3] With the owner: quickstart step 2 (SC-002 timing); record in
  `hardware-validation.md`.

**Checkpoint**: previews work and never touch roll folders or film position.

---

## Phase 6: User Story 4 - Choose a quality preset (Priority: P2)

**Goal**: `--preset web|standard|high|max` or `--resolution DPI` on `scan` and `roll`, and
`primefilm presets` to list them.

**Independent Test**: scanning the same frame at each preset gives the data-model pixel sizes
and the preset name in the sidecar.

### Tests for User Story 4

- [X] T042 [P] [US4] CLI tests in `tests/cli.rs`: `presets` lists 4 presets with `standard`
  marked default and the data-model sizes; `presets --json` matches the schema's `presets`
  items; `scan --preset web` records `"preset": "web"` and the fake's web size; `--resolution
  2400` records `custom` and 2400; `--preset web --resolution 2400` → exit 2; `--preset huge`
  → exit 2 listing valid presets; `--resolution 30000` → exit 2; none of these exit-2 cases
  touches the (fake) scanner.

### Implementation for User Story 4

- [X] T043 [US4] In `src/cli.rs`: `--preset` (clap `ValueEnum` over `Preset`) and
  `--resolution DPI` (range 25–10000), mutually exclusive, on `scan` and `roll`; the
  `presets` subcommand.
- [X] T044 [US4] Thread the chosen preset or custom resolution through `scan_single` and
  `scan_roll` in `src/scan.rs` into `ScanSettings` and `ScanRecord` (`preset: custom` for
  `--resolution`).
- [X] T045 [US4] Render the preset table (name, dpi, pixels, file size, seconds per frame or
  "not yet measured", default marker) and its JSON in `src/report.rs`; dispatch
  `Command::Presets` in `src/lib.rs` without opening any backend.

**Checkpoint**: all presets selectable and listed; default remains `standard`.

---

## Phase 7: User Story 5 - Advance film without scanning (Priority: P3)

**Goal**: `primefilm advance` moves the film one frame, logs it, writes no file.

**Independent Test**: preview, advance, preview: the second preview shows the next frame.

**Requires**: T004–T005 succeeded (film transport works).

### Tests for User Story 5

- [X] T046 [P] [US5] Tests: in `src/scan.rs`, `advance` moves the fake film `position` by
  exactly one and the next fake frame's first sample is the next number, and on the last
  frame reports `end-of-film` without moving; in `tests/cli.rs`,
  `--simulate ready advance --output D` → exit 0, prints "Film advanced one frame.", creates
  no files; `--simulate no-film advance` → exit 8.

### Implementation for User Story 5

- [X] T047 [US5] Add the `advance` subcommand (`--device`) to `src/cli.rs` and implement
  `advance` in `src/scan.rs` (research D5): two `Throwaway` scans (25 dpi, `Rgb8`), the first
  with `advance_after = true`, the second with `advance_after = false`, discarding the data
  but keeping each fingerprint; empty gate → `NoFilm`; matching fingerprints → outcome
  `end-of-film`, "Already at the last frame; the film did not move."; otherwise log
  `film advanced`. Dispatch in `src/lib.rs`.
- [X] T048 [US5] With the owner: quickstart step 4; record in `hardware-validation.md`.

**Checkpoint**: all five stories work independently.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T049 [P] Update `README.md`: a "Scanning" section with each command, the output folder
  layout and naming, the eject-button note, presets and disk use per roll, the `/tmp` note
  (research R3), resuming a roll, and the new exit codes 8–10.
- [ ] T050 [P] Re-run feature 001's hardware checks (`detect`, `capabilities`, `cargo test --
  --ignored`) with the new `pieusb.conf` flags (research D2); record in
  `specs/002-film-scan/hardware-validation.md`.
- [X] T051 With the owner: run E8 (one RGBI 16-bit frame at each preset, timed) via `primefilm
  scan --preset …`, fill `Preset::seconds_per_frame` in `src/preset.rs` from the results,
  confirm SC-003 (standard ≤ half of max), and record in research.md and
  `hardware-validation.md`.
- [ ] T052 Run the whole quickstart.md (without and with the scanner, steps 1–9), confirm
  `primefilm detect` succeeds right after every step (SC-006), and mark each row in
  `hardware-validation.md`.
- [X] T053 Learning review: read through every new module with the owner's eye; check each
  concept in plan.md's list has its comment at first use, no `unwrap`/`expect` outside tests
  without a proof comment, every `unsafe` block has `// SAFETY:`; final `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none.
- **Foundational (Phase 2)**: after Setup; blocks all stories. T003 → T004 → T005 → T006 → T007
  (owner present). T008–T014 can start alongside the spike. T015 → T016 (same file; T016 also
  needs T006's findings and T013, T014). T017 needs T013; T018 alongside T017; T019 needs
  T010–T013.
- **US2 (Phase 3)**: after Phase 2. MVP.
- **US1 (Phase 4)**: after US2 (reuses `capture_frame` path and the scan CLI patterns) and only
  if T004–T006 showed working transport.
- **US3 (Phase 5)**: after Phase 2; independent of US1/US2 except shared files.
- **US4 (Phase 6)**: after US2 (adds flags to `scan`); touches `roll` if US1 is done.
- **US5 (Phase 7)**: after Phase 2 and working transport.
- **Polish (Phase 8)**: after the desired stories.

### Within Each User Story

- Tests first, and they fail before implementation.
- CLI definition → sequencing in `scan.rs` → dispatch and reporting → hardware validation.

### Parallel Opportunities

- Phase 2: T008, T009, T010, T011, T012, T014 are separate files and independent; T017, T018,
  T020, T021 likewise once T013 exists.
- Each story's test tasks marked [P] (different files) together.
- US3 can proceed in parallel with US2 or US1 apart from shared edits to `src/cli.rs`,
  `src/scan.rs` and `src/lib.rs`, so serialize those edits.

---

## Parallel Example: Phase 2 building blocks

```bash
Task: "Error variants NoFilm/ScanFailed/OutputError in src/error.rs"           # T008
Task: "Preset enum and sizes in src/preset.rs"                                 # T009
Task: "Streaming TIFF writer in src/tiff.rs"                                   # T010
Task: "FrameStats and empty-gate check in src/frame_stats.rs"                  # T011
Task: "Roll folders, naming, PartialFile publishing in src/output.rs"          # T012
Task: "SANE scan FFI declarations in src/backend/sane/ffi.rs"                  # T014
```

## Parallel Example: User Story 2

```bash
Task: "scan CLI tests in tests/cli.rs"                                          # T022
Task: "scan_single unit tests in src/scan.rs"                                   # T023
Task: "SANE test-backend scan test in tests/sane_test_backend.rs"               # T024
Task: "#[ignore] hardware scan test in tests/hardware.rs"                       # T025
```

---

## Implementation Strategy

### MVP First (User Story 2)

1. Phase 1, then Phase 2, including the hardware spike: it decides whether rolls are possible
   with pieusb at all.
2. Phase 3 (US2): `primefilm scan` saves archival frames from the real scanner.
3. **Stop and validate** on the XA (T029).

### Incremental Delivery

1. US2 single frame (MVP) → US1 roll (the owner's most important use) → US3 preview → US4
   presets → US5 advance → Polish.
2. If the spike shows pieusb cannot move the film, deliver US2, US3 and US4, mark US1 and US5
   blocked in this file, and propose a direct-USB backend feature.

### Learning Path

Follows plan.md's concept order: ownership moves and drop order (T015), associated types
(T013), `Write`/`Seek` and byte order (T010), file system and hard links (T012), reused
mutable buffers (T019), fixed-point casts (T016), `Option` defaults (T027), temp-dir
integration tests (T022). Read each finished task's diff and its concept comments before
moving on.

---

## Notes

- [P] tasks touch different files and have no dependencies on incomplete tasks.
- Commit after each task or logical group, on branch `002-film-scan`.
- Every film movement must come from `roll` or `advance` and be logged (FR-013).
