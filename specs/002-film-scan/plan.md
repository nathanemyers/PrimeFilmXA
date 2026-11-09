# Implementation Plan: Film Scanning (Single Frame, Roll, Preview)

**Branch**: `002-film-scan` | **Date**: 2026-11-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/002-film-scan/spec.md`

## Summary

Add five subcommands to `primefilm`: `scan` (one frame, film stays put), `roll` (scan, advance,
repeat until the film ends), `preview` (quick low-resolution look at the current frame),
`advance` (move one frame without saving) and `presets` (list `web` 1800, `standard` 3600
(default) and `high` 5000 dpi; a planned `max` 10000 dpi was dropped after hardware testing
showed it gives stretched images). Ejecting is left to the scanner's physical
button.

Full scans are RGBI at 16 bits and are streamed straight from SANE into a hand-written,
uncompressed TIFF (RGB plus an infrared channel, as VueScan writes them), with a JSON sidecar
recording every setting. Files are written under a hidden `.partial` name and published with a
hard link, which cannot replace an existing file. Film movement relies on pieusb's slide
transport, which our shipped `pieusb.conf` currently leaves disabled; enabling it and learning
how the XA behaves at the end of a strip are the first, hardware-gated steps
([research.md](research.md), E1–E8). The scanning sequence is written once against an extended
backend interface (a session with configure, start frame, read, end frame) and runs unchanged
against the real scanner, SANE's test backend and a fake film strip.

## Technical Context

**Language/Version**: Rust stable 1.99 (edition 2024), as in feature 001

**Primary Dependencies**: existing `clap` 4, `serde` + `serde_json`, `thiserror` 2, `ctrlc` 3;
new `jiff` 0.2 (local time for names and metadata). Dev-only: `tiff` 0.11 (independent decoder
to check our TIFF writer). System: `libsane1` / `libsane-dev` (adds `sane_start`,
`sane_get_parameters`, `sane_read`, `sane_cancel` and option setting to the FFI). See
[research.md](research.md) D11.

**Storage**: files. TIFF + JSON sidecar per scan in `<output>/<roll>/`, previews in
`<output>/previews/`; default output root `$HOME/Pictures/primefilm`.

**Testing**: `cargo test`: unit tests (TIFF writer, naming, presets, empty-gate statistics,
outcome mapping); CLI integration tests against the fake film strip (`--simulate`,
`--simulate-frames`); real FFI scan path against SANE's `test` backend; `#[ignore]` hardware
tests for preview, single scan and a short roll.

**Target Platform**: Ubuntu Linux x86_64, normal user, PrimeFilm XA on USB

**Project Type**: CLI tool (single crate, library + binary)

**Performance Goals**: preview under 15 s (SC-002); `standard` takes at most half the time of
the highest preset per frame (SC-003); our own memory use bounded by a fixed read buffer (about 1 MiB)
regardless of frame size.

**Constraints**: no overwrite, ever (FR-010); no complete-looking partial files (FR-011); film
moves only for `roll` and `advance` (FR-013/014); scanner released on every exit path
(FR-016); no retries after disconnects or scanner errors (FR-017); non-interactive (FR-018).

**Scale/Scope**: one scanner, rolls up to 999 frames (in practice up to ~80 for half-frame
rolls); about 1,500 new lines of Rust including comments and tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How this plan complies |
|-----------|--------|------------------------|
| I. Archival Fidelity | ✅ | Every full scan is RGBI 16-bit, uncompressed TIFF, with pieusb's dust removal and IR correction off (raw). Presets change resolution only. Written as `.partial`, published by hard link only when complete; existing files never replaced. Sidecar JSON records preset, resolution, mode, depth, every option value (exposure, gain, offset), scanner identity and timestamp; key fields are also TIFF tags. Previews are clearly non-archival, in their own folder. |
| II. Hardware Safety | ✅ | Options are per-session values (nothing persistent is written). Film moves only for `roll` and `advance`, and every movement is logged. `advance` is set explicitly on every scan. Transport is enabled only after experiments E1–E3 show `SLIDE INIT` does not move a loaded strip. Disconnects and scanner errors stop the run once, with a resume hint; no retries. Ctrl-C finishes the current frame, then releases; `Drop` releases on every path. |
| III. Swappable Backend | ✅ | All scanning goes through `ScannerBackend::open` → `ScanSession`; the roll/single/preview logic in `scan.rs` never touches SANE. The fallback if pieusb cannot move the XA's film (a direct-USB backend) would slot in behind the same traits. Hardware findings are recorded in research.md and hardware-validation.md. |
| IV. Testable Without Hardware | ✅ | Fake film strip with position tracking and failure scenarios; SANE `test` backend covers the real FFI scan path; TIFF output checked by an independent decoder; hardware tests `#[ignore]`d. |
| V. Simple, Scriptable First | ✅ | Flags only; JSON for every command; distinct exit codes; progress on stderr, results on stdout. No configuration file; presets are a fixed list; no compression or image processing (YAGNI). |
| VI. Learning by Building | ✅ | One new runtime crate, justified. TIFF writing is our own readable code. Removes 001's only explicit lifetime. New `unsafe` stays in `backend/sane/` with `// SAFETY:` comments. Concepts listed below. |

**Platform & Workflow**: Linux only, no root, crates.io + Ubuntu packages ✅. Hardware-touching
tasks are verified on the scanner and recorded in `hardware-validation.md` ✅.

**Post-design re-check (after Phase 1)**: still passing. The design adds an associated type to
the backend trait (needed so each backend can return its own session type, see Complexity
Tracking) and changes the shipped `pieusb.conf` flags, which affects only this tool's private
SANE config.

## Rust Concepts Introduced

Building on feature 001's list; each gets an explanatory comment where it first appears.

1. Moving ownership into a function: `Sane::open(self, …)` consumes the context; struct
   fields drop in declaration order (replaces 001's `Device<'a>` lifetime)
2. Associated types in traits: `type Session: ScanSession`
3. The `std::io::Write` and `Seek` traits, `BufWriter`, and `File::sync_all`
4. Byte-level binary formats: `to_le_bytes`, `from_ne_bytes`, endianness (TIFF writer)
5. `std::fs` and `Path`/`PathBuf`: creating folders, `hard_link` for no-overwrite publishing,
   `read_dir` to find the next frame number
6. `impl` blocks with `const` items and `impl Display` for small enums (presets)
7. Mutable borrows in loops: reading chunks into one reused `&mut [u8]` buffer
8. `Option` combinators and `if let … else` for defaults (`--roll`, `--frame`)
9. Floating-point to fixed-point conversion and `as` casts (setting SANE `Fixed` options)
10. Integration tests with temporary directories (`std::env::temp_dir` + unique names)

## Project Structure

### Documentation (this feature)

```text
specs/002-film-scan/
├── plan.md                    # This file
├── research.md                # pieusb findings, hardware experiments E1–E8, decisions D1–D12
├── data-model.md              # Presets, settings, frame layout, roll, saved scan, outcomes
├── quickstart.md              # Build, simulate, and hardware validation steps
├── contracts/
│   ├── cli.md                 # scan, roll, preview, advance, presets; exit codes 8–10
│   └── scan-output.schema.json
├── checklists/requirements.md
├── hardware-validation.md     # Filled in during implementation
└── tasks.md                   # Created by /speckit-tasks
```

### Source Code (repository root)

```text
sane.d/pieusb.conf             # CHANGED: slide-transport flags for the XA (after E1)
src/
├── main.rs                    # CHANGED: Ctrl-C message "stopping after this frame"
├── lib.rs                     # CHANGED: dispatch new commands
├── cli.rs                     # CHANGED: scan/roll/preview/advance/presets, hidden test flags
├── error.rs                   # CHANGED: NoFilm (8), ScanFailed (9), OutputError (10)
├── report.rs                  # CHANGED: progress lines, run summary, scan JSON
├── preset.rs                  # NEW: Preset enum, sizes, listing
├── scan.rs                    # NEW: single / roll / preview / advance sequencing over ScanSession
├── output.rs                  # NEW: roll folders, naming, next frame, .partial + hard_link, sidecar
├── tiff.rs                    # NEW: streaming baseline TIFF writer
├── frame_stats.rs             # NEW: per-channel statistics and the empty-gate check
├── capability.rs, usb.rs      # unchanged
└── backend/
    ├── mod.rs                 # CHANGED: ScanSession trait, ScanSettings, FrameParams, open()
    ├── fake.rs                # CHANGED: simulated film strip and new scenarios
    └── sane/
        ├── ffi.rs             # CHANGED: sane_start/get_parameters/read/cancel, SET_VALUE, SaneParameters
        └── mod.rs             # CHANGED: Device owns Sane; set options; SaneSession; status mapping
tests/
├── cli.rs                     # CHANGED: scan commands via --simulate, files, exit codes, JSON
├── sane_test_backend.rs       # CHANGED: real scan through SANE's test backend
└── hardware.rs                # CHANGED: #[ignore] preview, scan, short roll
```

**Structure Decision**: same single crate as feature 001. New logic goes in small,
single-purpose modules (`preset`, `scan`, `output`, `tiff`, `frame_stats`) so each can be unit
tested and read on its own; only `backend/` knows about SANE.

## Implementation Order (for /speckit-tasks)

1. **Hardware spike first**: experiments E1–E8 with `scanimage` and the repo's SANE config;
   set the `pieusb.conf` flags; record findings. If transport fails, continue with steps 2–6
   for `scan` and `preview` only and raise a follow-up feature for the backend.
2. Pure modules with tests: `preset`, `tiff`, `frame_stats`, `output`.
3. Backend traits + fake film strip; `scan.rs` sequencing; CLI and reporting; CLI tests.
4. FFI and `SaneSession` (ownership change first, with 001's tests still passing), checked
   against SANE's test backend.
5. Real-scanner validation per quickstart.md, in the order preview → scan → advance → roll.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Associated type `type Session: ScanSession` on `ScannerBackend` (beyond "plain" traits, Principle VI) | Each backend returns its own session type, so the shared sequencing code in `scan.rs` stays generic and tested with the fake backend. | A trait object (`Box<dyn ScanSession>`) is the "trait-object gymnastics" Principle VI discourages; per-command backend methods would duplicate the sequencing in the fake backend, so tests would miss the real logic. |
| Changing the upstream `pieusb.conf` line for the XA | pieusb never moves film for a device without the slide-transport flag. | Asking the owner to edit `/etc/sane.d` needs root and affects other programs; a direct-USB backend is far more work and is kept as the fallback. |
