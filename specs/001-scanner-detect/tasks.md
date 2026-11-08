---

description: "Task list for 001-scanner-detect: Scanner Detection and Capability Listing"
---

# Tasks: Scanner Detection and Capability Listing

**Input**: Design documents from `/specs/001-scanner-detect/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/cli.md](contracts/cli.md),
[contracts/output.schema.json](contracts/output.schema.json), [quickstart.md](quickstart.md)

**Tests**: Included. FR-011 and constitution Principle IV require every non-hardware behavior to
be tested without the scanner. Hardware tests are `#[ignore]`d.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested
on its own.

**Rules for every task** (constitution v1.1.0):

- **Rust concept comments**: the first time a Rust concept from plan.md's "Rust Concepts
  Introduced" list appears, add a short comment explaining what it does and why it's used here.
- **`unwrap()`/`expect()`**: allowed only in tests, or with a comment proving the call cannot
  fail.
- **`unsafe`**: only under `src/backend/sane/`, and every `unsafe` block gets a `// SAFETY:`
  comment.
- **Done means**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` all pass.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1, US2, US3)

## Path Conventions

Single Cargo crate at the repository root: `src/`, `tests/`, `Cargo.toml`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: create the Cargo project and enforce the constitution's code rules with the
compiler.

- [X] T001 Create `Cargo.toml` at the repo root: `[package] name = "primefilm"`,
  `edition = "2024"`, `rust-version = "1.99"`, `[lib] path = "src/lib.rs"`,
  `[[bin]] name = "primefilm"`, `path = "src/main.rs"`. Dependencies: `clap = { version = "4",
  features = ["derive"] }`, `serde = { version = "1", features = ["derive"] }`,
  `serde_json = "1"`, `thiserror = "2"`, `ctrlc = "3"`. Add `[lints.clippy] unwrap_used =
  "warn"` and `expect_used = "warn"`, and create `clippy.toml` with `allow-unwrap-in-tests = true`
  and `allow-expect-in-tests = true` (Principle VI).
- [X] T002 Create the module skeleton with empty files: `src/lib.rs` declaring `pub mod cli;
  pub mod error; pub mod capability; pub mod report; pub mod usb; pub mod backend;`, plus
  `src/cli.rs`, `src/error.rs`, `src/capability.rs`, `src/report.rs`, `src/usb.rs`,
  `src/backend/mod.rs` (declaring `pub mod fake;` and `pub mod sane;`), `src/backend/fake.rs`,
  `src/backend/sane/mod.rs` (declaring `pub mod ffi; pub mod config;`),
  `src/backend/sane/ffi.rs`, `src/backend/sane/config.rs`, and a `src/main.rs` with an empty
  `fn main()`. Comment the crate/module/`pub` concepts in `src/lib.rs` (concept 1). Verify with
  `cargo build`.
- [X] T003 Enforce "unsafe only in the SANE wrapper": add `#![deny(unsafe_code)]` at the top of
  `src/lib.rs` and `src/main.rs`, and `#[allow(unsafe_code)]` on the `pub mod sane;` line in
  `src/backend/mod.rs`, with a comment pointing to constitution Principle VI.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: shared types, the CLI shape, error outcomes and raw SANE declarations that every
story uses.

**⚠️ CRITICAL**: no user story work can begin until this phase is complete.

- [X] T004 [P] Implement `ScanError` in `src/error.rs` with `thiserror` (concept 4), with
  variants exactly as in data-model.md: `NotFound`, `Busy { device: Option<String> }`,
  `PermissionDenied { path: String }`, `DriverUnavailable { reason: String }`,
  `Disconnected { during: String }`, `Interrupted`, `Sane { status: i32, text: String }`. Add
  methods:
  - `exit_code(&self) -> u8`: NotFound 3, Busy 4, PermissionDenied 5, DriverUnavailable 6,
    Disconnected 7, Interrupted 130, Sane 1. Use an exhaustive `match` (concept 3).
  - `outcome(&self) -> &'static str`: `"not-found"`, `"busy"`, `"permission-denied"`,
    `"driver-unavailable"`, `"disconnected"`, `"interrupted"`, `"error"`.
  - `next_step(&self) -> String`.

  Messages (`Display`) and next steps must be copied from the outcomes table in
  contracts/cli.md. Add `#[cfg(test)]` unit tests asserting every exit code and outcome string,
  and that all seven exit codes are distinct (SC-003).
- [X] T005 [P] Define `DeviceInfo { name: String, vendor: String, model: String, kind: String }`
  in `src/backend/mod.rs` (derive `Debug, Clone, PartialEq, Serialize, Deserialize`), plus
  `pub trait ScannerBackend { fn list_devices(&mut self) -> Result<Vec<DeviceInfo>, ScanError>; }`
  (concept 6). Document that `name` is "Non-empty; used with `--device`", and that `kind` is
  named that way because `type` is a Rust keyword.
- [X] T006 [P] Define the CLI in `src/cli.rs` with clap derive, per contracts/cli.md:
  - `Cli` fields: `verbose: u8` (`-v`, `ArgAction::Count`, global), `json: bool` (`--json`,
    global), `simulate: Option<Scenario>` (hidden, global), `sane_backend: Option<String>`
    (hidden, global), `command: Command`.
  - `enum Command { Detect, Capabilities { #[arg(long)] device: Option<String> } }`.
  - `#[derive(ValueEnum)] enum Scenario { Ready, NoScanner, Busy, PermissionDenied,
    DriverMissing, Disconnect, Interrupt }`, with kebab-case names as listed in the contract.

  Add a unit test using `Cli::try_parse_from` for `primefilm -vv --json capabilities --device X`
  and for global flags placed after the subcommand.
- [X] T007 [P] Write the raw SANE declarations in `src/backend/sane/ffi.rs` per research.md D2,
  mirroring `/usr/include/sane/sane.h`:
  - Type aliases `SaneWord = i32`, `SaneHandle = *mut c_void`.
  - `#[repr(C)]` structs `SaneDevice { name, vendor, model, type_: *const c_char }`,
    `SaneRange { min, max, quant: i32 }`, and `SaneOptionDescriptor { name, title, desc:
    *const c_char, type_, unit, size, cap, constraint_type: i32, constraint: *const c_void }`.
  - `pub const`s for `STATUS_*` 0–11, `TYPE_*` 0–5, `UNIT_*` 0–6, `CAP_*` (1, 2, 4, 8, 16, 32,
    64), `CONSTRAINT_*` 0–3, and `ACTION_GET_VALUE = 0`.
  - A `#[link(name = "sane")] unsafe extern "C"` block with `sane_init`, `sane_exit`,
    `sane_get_devices`, `sane_open`, `sane_close`, `sane_get_option_descriptor`,
    `sane_control_option` and `sane_strstatus`.

  Comment FFI, `extern "C"`, `#[repr(C)]` and raw pointers (concept 10), and explain why the
  C union is modeled as one `*const c_void`. Add a unit test asserting the struct sizes match C
  on x86_64 (`size_of::<SaneOptionDescriptor>() == 56`, `SaneRange == 12`, `SaneDevice == 32`).
- [X] T008 [P] Implement the SANE config writer in `src/backend/sane/config.rs` per research.md
  D3: `pub fn prepare(backend: &str, driver_debug: bool) -> Result<PathBuf, ScanError>`.
  - Choose the directory `$XDG_RUNTIME_DIR/primefilm/sane.d`, falling back to
    `std::env::temp_dir().join("primefilm-sane.d")`, and create it.
  - Write `dll.conf` containing just `backend`. When `backend == "pieusb"`, also write
    `pieusb.conf` from `include_str!("../../../sane.d/pieusb.conf")`.
  - Set `SANE_CONFIG_DIR` to the directory with no trailing colon. When `driver_debug`, also set
    `SANE_DEBUG_PIEUSB=5`. Both `std::env::set_var` calls are `unsafe` in edition 2024; add a
    `// SAFETY:` comment stating they run before any other thread exists. Make the function's
    doc comment say it must be called before the Ctrl-C handler is installed.
  - Map I/O failures to `ScanError::Sane { status: -1, text }`.
- [X] T009 Implement the run skeleton and output plumbing. Depends on T004–T006.
  - `src/report.rs`: `pub struct Logger { level: u8 }` with `fn step(&self, msg: &str)`, which
    prints `primefilm: {msg}` to stderr when `level >= 1` (no custom macros, per Principle VI).
  - `src/report.rs`: `pub fn print_error(err: &ScanError, json: bool)`. It always prints the
    message and next step to stderr. With `json` it also prints `{"schema_version": 1,
    "outcome", "message", "next_step"}` to stdout via a `#[derive(Serialize)]` struct
    (concept 8).
  - `src/lib.rs`: `pub fn run(cli: &Cli, log: &Logger) -> Result<(), ScanError>`, which returns
    `Ok(())` for now.
  - `src/main.rs`: parse `Cli`, build the `Logger`, call `run`, and on error call `print_error`
    and return `std::process::ExitCode::from(err.exit_code())`.

**Checkpoint**: `cargo test` passes; `cargo run -- --help` shows `detect` and `capabilities` and
hides `--simulate` / `--sane-backend`.

---

## Phase 3: User Story 1 - Check that the scanner is ready (Priority: P1) 🎯 MVP

**Goal**: `primefilm detect` reports the PrimeFilm XA (model name and connection identifier)
or a clear "not found", in text or JSON.

**Independent Test**: with the scanner on, `primefilm detect` exits 0 and names it; with it off,
it exits 3 with the power/cable next step. Without hardware: `--simulate ready` and
`--simulate no-scanner` give the same results.

### Tests for User Story 1

- [X] T010 [P] [US1] Create `tests/cli.rs` with a helper `fn primefilm(args: &[&str]) -> (i32,
  String, String)` that runs `env!("CARGO_BIN_EXE_primefilm")` through `std::process::Command`
  and returns exit code, stdout and stderr. Add tests:
  - `--simulate ready detect` → exit 0 and stdout contains `RPS 10M`.
  - `--simulate no-scanner detect` → exit 3 and stderr contains `No PrimeFilm XA found`.
  - `--json --simulate ready detect` → stdout parses with `serde_json`, `schema_version == 1`,
    `outcome == "found"`, `devices` has 1 entry with all four fields.
  - `--json --simulate no-scanner detect` → `outcome == "not-found"`, and `message` and
    `next_step` are non-empty.
- [X] T011 [P] [US1] Create `tests/sane_test_backend.rs` that runs the binary with
  `--sane-backend test detect` and asserts exit 0, that every listed device name starts with
  `test:`, and that no `v4l:` device appears (FR-002, acceptance scenario US1-3).
- [X] T012 [P] [US1] Create `tests/hardware.rs` with `#[ignore]` test `detect_finds_scanner`:
  run `primefilm detect`, assert exit 0 and stdout contains `pieusb:`. Add a file-level comment:
  "Needs the real PrimeFilm XA. Run with `cargo test -- --ignored`".

### Implementation for User Story 1

- [X] T013 [P] [US1] Implement `FakeBackend { scenario: Scenario }` in `src/backend/fake.rs`
  with `ScannerBackend::list_devices`:
  - `Ready`, `Disconnect`, `Interrupt` → `Ok(vec![DeviceInfo { name:
    "fake:primefilm-xa", vendor: "Reflecta", model: "RPS 10M", kind: "film scanner" }])`.
  - `NoScanner` → `Err(NotFound)`.
  - `Busy` → `Err(Busy { device: None })`.
  - `PermissionDenied` → `Err(PermissionDenied { path: "/dev/bus/usb/001/004" })`.
  - `DriverMissing` → `Err(DriverUnavailable { reason: "simulated: driver file missing" })`.
- [X] T014 [US1] Implement the safe SANE core in `src/backend/sane/mod.rs`:
  - `pub struct Sane` with `Sane::init() -> Result<Sane, ScanError>`, guarded by a `static
    INITIALIZED: AtomicBool`; a second init is an error (research D5).
  - `impl Drop for Sane` calling `sane_exit` and clearing the flag (concept 9).
  - `fn status_error(status: i32) -> ScanError`, using `sane_strstatus` to fill
    `ScanError::Sane`.
  - `fn c_str(ptr: *const c_char) -> String`: null → empty string; otherwise
    `CStr::to_string_lossy`.
  - `Sane::list_devices(&self) -> Result<Vec<DeviceInfo>, ScanError>`, calling
    `sane_get_devices(&mut list, 1)` and walking the NULL-terminated array.

  Every `unsafe` block gets a `// SAFETY:` comment.
- [X] T015 [US1] Implement `pub struct SaneBackend { backend: String }` with
  `ScannerBackend::list_devices` in `src/backend/sane/mod.rs`: init SANE, list devices, keep only
  names starting with `"{backend}:"`, and return `Err(ScanError::NotFound)` when the list is
  empty. US3 refines this diagnosis. Depends on T014.
- [X] T016 [P] [US1] Add detect rendering in `src/report.rs`:
  - `pub fn print_devices(devices: &[DeviceInfo], json: bool)`.
  - Human output, per contracts/cli.md: `Found N scanner(s):` then
    `  {name}  {vendor} {model} ({kind})`.
  - JSON output: `#[derive(Serialize)] struct DetectReport { schema_version: 1, outcome:
    "found", devices }`.
- [X] T017 [US1] Wire `detect` end to end. Depends on T013–T016.
  - In `src/lib.rs`, add a generic `fn detect<B: ScannerBackend>(backend: &mut B, json: bool,
    log: &Logger)`.
  - In `run`, `match` on `cli.simulate`: `Some(s)` → `FakeBackend`; `None` →
    `SaneBackend::new(cli.sane_backend.as_deref().unwrap_or("pieusb"))`. Comment why generics
    let both backends share one code path (concept 6).
  - In `src/main.rs`, call `config::prepare(backend, cli.verbose >= 2)` before `run` when not
    simulating.

**Checkpoint**: T010–T011 pass; `cargo run -- --simulate ready detect` works; with the scanner
on, `cargo test -- --ignored detect_finds_scanner` passes (risk R1).

---

## Phase 4: User Story 2 - See what the scanner can do (Priority: P2)

**Goal**: `primefilm capabilities` prints a summary of key film-scanning capabilities, then
every option with its value, constraint and flags; `--json` gives the same data.

**Independent Test**: `--simulate ready capabilities` shows the summary and grouped options;
`--sane-backend test --json capabilities` returns arrays, buttons, groups and inactive options
correctly; with the scanner on, every option matches `./scripts/probe.sh` (SC-002).

### Tests for User Story 2

- [X] T018 [P] [US2] Add unit tests in `src/capability.rs`:
  - `fixed_to_f64(65536) == 1.0` and `fixed_to_f64(-32768) == -0.5`.
  - `CapabilitySummary::from_options` on a hand-built list (`resolution` word list
    `[1800, 3600, 7200]`, `mode` strings `["Color", "Gray", "RGBI"]`, `depth` `[8, 16]`,
    `br-x` range max 36.0, `br-y` range max 24.0) → those values and `infrared == "available"`.
  - An empty list → all `None` and `infrared == "unknown"`.
  - A list whose modes lack RGBI/infrared → `"not available"`.
- [X] T019 [P] [US2] Add tests to `tests/cli.rs`:
  - `--simulate ready capabilities` → exit 0, stdout contains `Summary` and `Infrared`.
  - `--json --simulate ready capabilities` → `device`, `summary`, `options` present; every
    option has the 13 keys from data-model.md; at least one option has `"active": false` and
    `"value": null` (US2 scenario 3).
  - `--simulate ready capabilities --device nope` → exit 3.
- [X] T020 [P] [US2] Add a test to `tests/sane_test_backend.rs`: `--sane-backend test --json
  capabilities` → exit 0, and `options` together contain `value_type`s `int`, `fixed`, `bool`,
  `string`, `button` and `group`. Every `button`/`group` has `value: null`, and at least one
  `int` option's `value` array has more than 1 element (arrays not truncated, research D2).
- [X] T021 [P] [US2] Add an `#[ignore]` test `capabilities_lists_options` to
  `tests/hardware.rs`: `primefilm --json capabilities` → exit 0, `options` non-empty, and
  `summary.resolutions_dpi` not null.

### Implementation for User Story 2

- [X] T022 [P] [US2] Implement the capability types in `src/capability.rs` per data-model.md,
  all deriving `Debug, Clone, PartialEq, Serialize, Deserialize`:
  - `Capability`, with all 13 fields and the field names exactly as in data-model.md.
  - `ValueType` (`#[serde(rename_all = "lowercase")]`) and `Unit` (lowercase), each with a
    `from_sane(i32)` constructor. Unknown type → `Err`; unknown unit → `Unit::None`.
  - `Constraint` (`#[serde(tag = "kind")]` with variants renamed `none`, `range`, `list`,
    `strings`). "`max` is inclusive; `step` 0 means any value."
  - `Value` (`#[serde(untagged)]`: `Bool(Vec<bool>)`, `Int(Vec<i32>)`, `Fixed(Vec<f64>)`,
    `Text(String)`).
  - `pub fn fixed_to_f64(raw: i32) -> f64` returning `raw as f64 / 65536.0`.
- [X] T023 [US2] Implement `CapabilitySummary { resolutions_dpi: Option<Constraint>,
  color_modes: Option<Vec<String>>, bit_depths: Option<Vec<i64>>, scan_area_mm:
  Option<ScanArea { width, height }>, infrared: Infrared }` and `CapabilitySummary::from_options
  (&[Capability])` in `src/capability.rs` using iterators and closures (concept 7), following
  research.md D8. `Infrared` serializes as `"available" | "not available" | "unknown"`.
  Depends on T022.
- [X] T024 [P] [US2] Hand-author the simulated option list `src/backend/fixtures/primefilm-xa.json`
  (a JSON array of `Capability`):
  - Groups: "Scan Mode", "Geometry", "Advanced".
  - `mode` string list `Color`, `Gray`, `RGBI` (value `"Color"`).
  - `resolution` int dpi list `[1800, 3600, 7200]` (value `[3600]`).
  - `depth` int bit list `[8, 16]`.
  - `preview` bool.
  - `tl-x`, `tl-y`, `br-x`, `br-y` fixed mm ranges with maximums 36.0 and 24.0.
  - One inactive option `gamma-table`, an int array of size 1024 bytes with `value: null`.

  Every entry must have all 13 `Capability` fields and no extra keys, because JSON has no
  comments and the file must deserialize as `Vec<Capability>`. Record in research.md R4 that it
  is hand-authored and must be replaced with a real recording.
- [X] T025 [US2] Extend the trait in `src/backend/mod.rs` with `fn read_capabilities(&mut self,
  device: &str, cancel: &AtomicBool) -> Result<Vec<Capability>, ScanError>`. Implement it in
  `src/backend/fake.rs`:
  - `Ready`: parse the fixture with `include_str!` + `serde_json::from_str`. `expect` is allowed
    here, with a comment that the fixture is compiled in and covered by T019.
  - `Disconnect` → `Err(Disconnected { during: "reading options" })`.
  - `Interrupt` → `Err(Interrupted)`.
  - An unknown device name → `Err(NotFound)`.
  - Other scenarios → the same errors as `list_devices`.
- [X] T026 [US2] Implement device access in `src/backend/sane/mod.rs`. Depends on T014, T022.
  - `pub struct Device<'a> { handle: SaneHandle, _sane: &'a Sane }`. Add a comment explaining
    the lifetime and why it's the one justified explicit lifetime (plan Complexity Tracking,
    concept 11).
  - `Sane::open(&self, name: &str) -> Result<Device<'_>, ScanError>` via `CString` +
    `sane_open`.
  - `impl Drop for Device` calling `sane_close`.
  - `Device::options(&self, cancel: &AtomicBool) -> Result<Vec<Capability>, ScanError>`:
    1. Read the option count from descriptor 0 via `sane_control_option(GET_VALUE)` into an
       `i32`.
    2. For `1..count`, return `Err(Interrupted)` if `cancel` is set, otherwise get the
       descriptor. A null descriptor → `ScanError::Sane`.
    3. Convert the strings with `c_str` and the flags from `cap`.
    4. Convert the constraint. RANGE → `SaneRange`, with fixed conversion for `TYPE_FIXED`.
       WORD_LIST → the first word is the count. STRING_LIST → read until a null pointer. A null
       constraint pointer → `Constraint::None`.
    5. Read the value only if active and not button/group. Bool/int/fixed use a `Vec<i32>` of
       `size / 4` words; bool is `word != 0`. Strings use a `Vec<u8>` of `size` bytes plus
       `CStr::from_bytes_until_nul`.

    Never call any action except `ACTION_GET_VALUE` (FR-006).
- [X] T027 [US2] Implement `SaneBackend::read_capabilities` in `src/backend/sane/mod.rs`: init,
  list, check `device` is in the filtered list (otherwise `NotFound`), open, read options.
  Drop order closes the device before `sane_exit` (research D5). Depends on T026.
- [X] T028 [P] [US2] Add capabilities rendering in `src/report.rs`:
  - `print_capabilities(device: &DeviceInfo, summary: &CapabilitySummary, options:
    &[Capability], json: bool)`.
  - Human output per contracts/cli.md: a header line, a `Summary` block (`unknown` for `None`),
    then options under their group titles.
  - Columns are name, value (single-element arrays printed bare; `(inactive)` for inactive),
    choices, and title.
  - JSON: `#[derive(Serialize)] struct CapabilitiesReport { schema_version: 1, outcome:
    "found", device, summary, options }`.
- [X] T029 [US2] Wire `capabilities [--device]` in `src/lib.rs`. Depends on T023, T025, T027,
  T028.
  - Add a generic `fn capabilities<B: ScannerBackend>(...)`.
  - Choose the device: the `--device` value if given, else the first from `list_devices`. When
    there is more than one device and no `--device`, write `using first scanner: {name}` to
    stderr.
  - Read the options, build the summary, and print.
  - For now, pass a `cancel` `AtomicBool` that is never set; US3 connects it to Ctrl-C.

**Checkpoint**: T018–T020 pass; `cargo run -- --sane-backend test capabilities` shows real SANE
test options; with the scanner on, compare against `./scripts/probe.sh` (SC-002, risks R2/R4).

---

## Phase 5: User Story 3 - Understand why detection failed (Priority: P3)

**Goal**: distinct messages, next steps and exit codes for not found, busy, permission denied,
driver unavailable, disconnected and interrupted; `-v`/`-vv` show what the tool did.

**Independent Test**: every `--simulate` failure scenario yields its own exit code and JSON
`outcome`; on hardware, VueScan holding the scanner → 4, unplug during `capabilities` → 7,
Ctrl-C → 130 and the scanner is free afterwards.

### Tests for User Story 3

- [X] T030 [P] [US3] Add unit tests in `src/usb.rs`. Each builds a fake sysfs tree under
  `std::env::temp_dir().join(format!("primefilm-usb-test-{}-{name}", std::process::id()))` with
  files `idVendor`, `idProduct`, `busnum`, `devnum`, and removes it at the end. Cases:
  - No matching device → `None`.
  - `05e3`/`0144` present → bus 1 and device 4 parsed from `"001"`/`"4"`.
  - An interface dir `1-2:1.0/driver` symlinked (`std::os::unix::fs::symlink`) to a path ending
    in `usbfs` → claimed is `true`.
  - No driver link → `false`.
- [X] T031 [P] [US3] Add tests to `tests/cli.rs`:
  - `--simulate busy detect` → exit 4 and stderr contains `in use`.
  - `permission-denied` → 5.
  - `driver-missing` → 6.
  - `--simulate disconnect capabilities` → 7.
  - `--simulate interrupt capabilities` → 130.
  - For each, `--json` gives the outcome string from contracts/cli.md, non-empty
    `message`/`next_step`, and stdout parses as JSON.
  - A test that the five exit codes plus 3 are all distinct (SC-003).
  - `-v --simulate ready detect` → stderr contains `primefilm:`.

### Implementation for User Story 3

- [X] T032 [P] [US3] Implement the `/sys` probe in `src/usb.rs` per research.md D4:
  - `pub struct UsbProbe { sysfs_root: PathBuf }` with `UsbProbe::system()` (root
    `/sys/bus/usb/devices`) and `UsbProbe::at(path)` for tests.
  - `find(&self, vendor: &str, product: &str) -> Option<UsbDevice { sys_name, bus: u32,
    device: u32 }>`, which trims file contents and skips unreadable entries.
  - `is_claimed(&self, dev: &UsbDevice) -> bool`, true when the `read_link` of
    `{sys_name}:1.0/driver` has file name `usbfs`.
  - `node_path(dev) -> PathBuf`, formatted as `/dev/bus/usb/{bus:03}/{device:03}`.
  - `check_node_access(dev) -> std::io::Result<()>`, which opens the node with
    `OpenOptions::new().read(true).write(true)` and drops it immediately. Comment that this
    does not claim the interface.
- [X] T033 [US3] Add diagnosis to `SaneBackend` in `src/backend/sane/mod.rs`, in the order of
  contracts/cli.md's outcomes table. Skip all `/sys` checks when the backend is not `pieusb`.
  Depends on T032. Before `Sane::init`:
  1. `libsane-pieusb.so.1` missing from all of `/usr/lib/x86_64-linux-gnu/sane`,
     `/usr/lib/sane` and `/usr/local/lib/sane` → `DriverUnavailable { reason: "driver file
     missing" }`.
  2. `05e3:0144` not found → `NotFound`.
  3. Claimed → `Busy`.
  4. Node access `PermissionDenied` → `PermissionDenied { path }`.

  After listing:
  - Empty list while the USB device is present → `DriverUnavailable { reason: "scanner on USB
    but pieusb did not recognize it (check the model number in sane.d/pieusb.conf; run with
    -vv)" }`.

  Status mapping in open/read:
  - `STATUS_DEVICE_BUSY` → `Busy`.
  - `STATUS_ACCESS_DENIED` → `PermissionDenied`.
  - Any failure during `options` → re-probe USB; gone → `Disconnected { during: "reading
    options" }`, else `status_error`.

  Never retry (Principle II).
- [X] T034 [US3] Add verbose steps via `Logger::step` in `src/lib.rs` and
  `src/backend/sane/mod.rs`, matching the examples in contracts/cli.md:
  - The config dir and backend.
  - The USB location and claimed state.
  - `sane_init ok (SANE x.y.z)`, decoded from the version code: major = `code >> 24`,
    minor = `(code >> 16) & 0xff`, build = `code & 0xffff`.
  - The device count.
  - The option count when opening.

  `SaneBackend::new` takes a `Logger`.
- [X] T035 [US3] Install Ctrl-C handling in `src/main.rs` (concept 12):
  - Create `Arc<AtomicBool>`. After `config::prepare`, call `ctrlc::set_handler` with a closure
    that stores `true`. Comment why `Arc` and an atomic are needed to share it with the
    handler's thread.
  - Pass the flag into `run` and on to `read_capabilities`, replacing T029's placeholder.
  - Confirm `Interrupted` prints `Interrupted; scanner released.` and exits 130 after the
    device and SANE have been dropped.

**Checkpoint**: T030–T031 pass; all seven outcomes are reachable via `--simulate`; hardware
checks 6–8 in quickstart.md pass (risk R3).

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: documentation, learning-quality review and hardware validation.

- [X] T036 [P] Write `README.md` at the repo root: purpose, prerequisites (rustup, `libsane-dev`),
  build, usage of `detect` and `capabilities` (with and without `--json`), and a link to the exit
  codes in `specs/001-scanner-detect/contracts/cli.md`. Include a "Permissions" section with the
  udev rule `SUBSYSTEM=="usb", ATTR{idVendor}=="05e3", ATTR{idProduct}=="0144", MODE="0666"`
  for `/etc/udev/rules.d/60-primefilm.rules`, which the permission-denied next step refers to.
- [X] T037 Learning review of every file under `src/` (Principle VI):
  - Each concept in plan.md's "Rust Concepts Introduced" list has an explanatory comment at its
    first use.
  - Every `unsafe` block has `// SAFETY:`.
  - `grep -rn "unwrap()\|expect(" src/` shows only test code or commented, provably safe uses.
  - Add a short "How the code is organized" section to `README.md` that walks the modules in
    reading order.
- [X] T038 Run the gates over the whole crate (`Cargo.toml`, `src/`, `tests/`):
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`; fix
  anything that fails.
- [ ] T039 Run the hardware validation in `specs/001-scanner-detect/quickstart.md` steps 1–9
  with the scanner on, and record the date, film loaded and pass/fail per step in a new file
  `specs/001-scanner-detect/hardware-validation.md`. Update research.md risks R1–R4 with what
  was found, and fix `sane.d/pieusb.conf` if the model byte differs (R1).
- [ ] T040 Replace the hand-authored fixture with a real recording (quickstart step 10).
  Extract the `options` array from `cargo run -q -- --json capabilities` into
  `src/backend/fixtures/primefilm-xa.json`, adjust the summary heuristics in
  `src/capability.rs` if the real option names differ (research D8), and re-run `cargo test`.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 → T002 → T003.
- **Foundational (Phase 2)**: needs Setup. T004–T008 are independent ([P]); T009 needs T004–T006.
- **US1 (Phase 3)**: needs Foundational.
- **US2 (Phase 4)**: needs Foundational and US1's SANE core (T014, T015) and the backend
  selection in T017.
- **US3 (Phase 5)**: needs US1 (detect path) and US2 (`read_capabilities`, for disconnect and
  interrupt).
- **Polish (Phase 6)**: needs all stories. T039–T040 also need the scanner powered on.

### User Story Dependencies

- **US1 (P1)**: independent after Foundational, and delivers the MVP.
- **US2 (P2)**: builds on US1's `Sane` wrapper and `run` dispatch, but is tested independently
  through `--simulate ready` and `--sane-backend test`.
- **US3 (P3)**: adds diagnosis on top of both. Its fake-backend tests need only the CLI paths
  from US1/US2.

### Within Each User Story

- Write the story's tests first and see them fail, then implement.
- Types before backends, backends before rendering, wiring last.
- Run the three cargo gates before checking off a task.

### Parallel Opportunities

- Foundational: T004, T005, T006, T007, T008 all touch different files.
- US1: tests T010, T011, T012 together; T013 and T016 alongside T014.
- US2: tests T018–T021 together; T022, T024, T028 in parallel; T026 after T022.
- US3: T030, T031, T032 in parallel.
- Polish: T036 alongside T037.

## Parallel Example: User Story 2

```bash
# Tests first, all different files:
Task: "Unit tests for fixed conversion and summary in src/capability.rs"        # T018
Task: "Capabilities CLI tests with --simulate ready in tests/cli.rs"            # T019
Task: "SANE test-backend capabilities test in tests/sane_test_backend.rs"       # T020
Task: "#[ignore] hardware capabilities test in tests/hardware.rs"               # T021

# Then independent implementation pieces:
Task: "Capability types in src/capability.rs"                                   # T022
Task: "Hand-authored fixture src/backend/fixtures/primefilm-xa.json"            # T024
Task: "Capabilities rendering in src/report.rs"                                 # T028
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Phase 1 Setup, then Phase 2 Foundational.
2. Phase 3 (US1): `primefilm detect` works on the fake backend, SANE `test`, and the real
   scanner.
3. **Stop and validate**: run `primefilm detect` with the scanner on and off. This also settles
   risk R1 (does pieusb recognize the XA?) before investing in US2.

### Incremental Delivery

1. Setup + Foundational → project builds and the CLI shape exists.
2. US1 → a "is it there?" check (MVP).
3. US2 → the full capability listing, which informs future scan features.
4. US3 → precise diagnostics for the failures already seen (busy, disconnects).
5. Polish → documentation, learning review and recorded hardware validation.

### Learning Path

Tasks follow plan.md's concept order:
- **Setup and Foundational**: modules, enums, `match` and `Result` (T001–T009).
- **US1**: traits and generics, FFI and `Drop` (T013–T017).
- **US2**: iterators, serde, and the one lifetime (T022–T029).
- **US3**: threads and atomics (T035).

Read each finished task's diff and its concept comments before moving on.

---

## Notes

- [P] tasks touch different files and have no dependencies on incomplete tasks.
- Commit after each task or logical group, on branch `001-scanner-detect`.
- The scanner must never be asked to scan, move film or change a setting in this feature
  (FR-006).
