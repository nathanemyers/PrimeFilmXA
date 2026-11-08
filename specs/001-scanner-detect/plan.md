# Implementation Plan: Scanner Detection and Capability Listing

**Branch**: `001-scanner-detect` | **Date**: 2026-11-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-scanner-detect/spec.md`

## Summary

Build `primefilm`, a Rust command-line tool with two subcommands: `detect` reports whether the
PrimeFilm XA is connected and usable, and `capabilities` lists every setting the scanner
exposes, preceded by a summary of the film-scanning essentials. Both support `--json`.

The tool talks to the scanner through SANE's C library using a small, hand-written FFI layer
(8 functions), loading only the `pieusb` driver so no other driver can claim the scanner first.
Failures are diagnosed by combining SANE status codes with the Linux USB view in `/sys`, giving
distinct messages and exit codes for not found, busy, permission denied, driver unavailable,
disconnected and interrupted. All scanner access sits behind a `ScannerBackend` trait with a
real SANE implementation and a fake one, so everything except the hardware itself is tested
without the scanner. The code is written to teach Rust (constitution Principle VI).

## Technical Context

**Language/Version**: Rust stable 1.99 (edition 2024), installed via rustup

**Primary Dependencies**: `clap` 4 (CLI parsing, derive), `serde` 1 + `serde_json` 1 (JSON output
and fixture loading), `thiserror` 2 (error enum), `ctrlc` 3 (Ctrl-C handling). System:
`libsane1` + `libsane-dev` 1.4.0 (linked via `#[link(name = "sane")]`; no build script, no
bindgen). See [research.md](research.md) for justifications.

**Storage**: N/A. The only file written is SANE's runtime config
(`$XDG_RUNTIME_DIR/primefilm/sane.d/`), regenerated each run.

**Testing**: `cargo test`: inline unit tests; integration tests that run the built binary
against the fake backend (`--simulate`) and SANE's bundled `test` backend
(`--sane-backend test`); real-scanner tests marked `#[ignore]` (`cargo test -- --ignored`).

**Target Platform**: Ubuntu Linux (kernel 7.x), x86_64, run as a normal user

**Project Type**: CLI tool (single crate with a library and a binary)

**Performance Goals**: `detect` answers within 5 seconds with the scanner on (SC-001)

**Constraints**: strictly read-only towards the scanner (FR-006); scanner released on every
exit path (FR-007); never prompts (FR-009); no root at runtime

**Scale/Scope**: one user, one scanner, ~1,000 lines of Rust including comments and tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How this plan complies |
|-----------|--------|------------------------|
| I. Archival Fidelity | N/A | No image data is captured or written in this feature. |
| II. Hardware Safety | ✅ | Only `GET_VALUE` is ever sent; no scan start, no option set. SANE handle and context are released by `Drop` on every path, including Ctrl-C. Disconnects are reported once, never retried. |
| III. Swappable Scanner Backend | ✅ | `ScannerBackend` trait with `SaneBackend` and `FakeBackend`; nothing else touches SANE. Hardware findings go in [research.md](research.md). |
| IV. Testable Without Hardware | ✅ | Fake backend covers every outcome; SANE's `test` backend exercises the real FFI code; hardware tests are `#[ignore]`d. |
| V. Simple, Scriptable First | ✅ | Two subcommands, flags only, JSON option, errors on stderr, distinct exit codes. No config files or plugins. |
| VI. Learning by Building | ✅ | Plain Rust; concept comments on first use; four widely used crates, each justified; no `unwrap` outside tests; `unsafe` only in `src/backend/sane/` with `// SAFETY:` comments. One explicit lifetime, justified below. |

**Platform & Hardware Constraints**: Linux only, `05e3:0144`, no root, Rust via rustup,
dependencies from crates.io + Ubuntu packages ✅. **Development Workflow**: `cargo fmt --check`,
`cargo clippy -- -D warnings`, `cargo test` gate every task ✅.

**Post-design re-check (after Phase 1)**: still passing. The design added a hidden
`--sane-backend` flag and the `/sys` probe; both serve Principle IV and II and add no new
abstraction layers.

## Rust Concepts Introduced

Ordered so tasks can build on one another; each gets an explanatory comment where it first
appears in the code.

1. Cargo project, crates, `mod` and `pub` (module tree in `lib.rs`)
2. Structs and enums, `#[derive(...)]`
3. `match` and exhaustive enum handling (exit-code mapping)
4. `Result`, the `?` operator, custom error enums with `thiserror`
5. Ownership and borrowing: `String` vs `&str`, `Vec<T>` vs `&[T]`
6. Traits and generics (`fn run<B: ScannerBackend>(backend: &mut B)`)
7. Iterators and closures (`iter().filter().map()` in the summary)
8. Serde derive for JSON (`Serialize` / `Deserialize`)
9. `Drop` and RAII: resources freed automatically when values go out of scope
10. FFI: `extern "C"`, `#[repr(C)]`, raw pointers, `CStr`/`CString`, `unsafe` and `// SAFETY:`
11. One lifetime parameter: `Device<'a>` cannot outlive the `Sane` context it came from
12. Shared state across threads: `Arc<AtomicBool>` set by the Ctrl-C handler
13. Testing: unit tests (`#[cfg(test)]`), integration tests (`tests/`), `#[ignore]`

## Project Structure

### Documentation (this feature)

```text
specs/001-scanner-detect/
├── plan.md               # This file
├── research.md           # Phase 0: decisions, rationale, risks
├── data-model.md         # Phase 1: entities and their rules
├── quickstart.md         # Phase 1: how to build, run and validate
├── contracts/
│   ├── cli.md            # Command-line contract: commands, flags, exit codes, output
│   └── output.schema.json  # JSON Schema for --json output
├── checklists/
│   └── requirements.md   # Spec quality checklist
└── tasks.md              # Created by /speckit-tasks
```

### Source Code (repository root)

```text
Cargo.toml
sane.d/                        # Existing: pieusb-only SANE config, embedded into the binary
src/
├── main.rs                    # Parse CLI, prepare SANE config, install Ctrl-C, run, exit code
├── lib.rs                     # Module tree and run()
├── cli.rs                     # clap definitions
├── error.rs                   # ScanError: outcomes → message, next step, exit code
├── capability.rs              # Capability, Value, Constraint, Unit, CapabilitySummary
├── report.rs                  # Human-readable and JSON rendering
├── usb.rs                     # /sys probe: present, claimed, device node permissions
└── backend/
    ├── mod.rs                 # ScannerBackend trait, DeviceInfo
    ├── fake.rs                # FakeBackend and its scenarios
    ├── fixtures/
    │   └── primefilm-xa.json  # Simulated scanner options
    └── sane/
        ├── ffi.rs             # C declarations from sane.h (raw, unsafe)
        ├── config.rs          # Write sane.d, set SANE_CONFIG_DIR
        └── mod.rs             # Safe wrapper (Sane, Device<'a>) and SaneBackend
tests/
├── cli.rs                     # Every outcome via --simulate; JSON parses; exit codes
├── sane_test_backend.rs       # Real FFI against SANE's bundled `test` backend
└── hardware.rs                # #[ignore]: needs the real scanner
```

**Structure Decision**: a single Cargo crate with a library (`src/lib.rs`) and a binary
(`src/main.rs`). The library split lets unit tests reach internal modules, while integration
tests run the real binary. `scripts/probe.sh` stays as an independent cross-check.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Explicit lifetime `Device<'a>` (Principle VI discourages explicit lifetimes) | SANE forbids using a device handle after `sane_exit`. Tying `Device<'a>` to `&'a Sane` makes the compiler reject that bug. | Opening and closing inside one function hides the rule instead of teaching it, and a runtime check would turn a compile error into a crash. |
