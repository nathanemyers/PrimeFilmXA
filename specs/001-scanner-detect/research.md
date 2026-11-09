# Research: Scanner Detection and Capability Listing

**Feature**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md) | **Date**: 2026-11-08

## Findings from the hardware (2026-11-08)

- The scanner enumerates as USB `05e3:0144` (Genesys Logic), USB 2.0 high speed, vendor-specific
  class, one interface with bulk endpoints `0x81` IN and `0x02` OUT. No manufacturer or product
  strings. Device node permissions were `crw-rw-rw-` (usable without root).
- SANE 1.4.0 (`libsane1`) ships `/etc/sane.d/pieusb.conf` with the line
  `usb 0x05e3 0x0144 0x0048 0x00`, labeled "Reflecta RPS 10M (aka Pacific Image PrimeFilm XAs)".
- `scanimage -L` with all backends: pieusb reported `sanei_usb_open failed ... Device busy`,
  and the interface's driver link in `/sys` pointed to `usbfs`, meaning something had claimed it.
  The likely cause is another SANE backend opening it earlier in the same process.
- The scanner dropped off USB twice within minutes (`usb 1-2: USB disconnect`), once during a
  probe. The cause is unknown (idle power-off, cable, or the probe itself).
- SANE's `test` backend (`libsane-test.so`) and the `libsane.so` development symlink are
  installed. pkg-config reports `sane-backends` as `1.2.1-debian` (a stale version string in
  Ubuntu's `.pc` file).

## Findings from the hardware (2026-11-09)

- **colord claimed the scanner and caused the 90-second disconnects.** On every connect, the
  `colord` service started its helper `/usr/libexec/colord-sane` (parent: `colord`, cgroup
  `system.slice/colord.service`). The helper loads every SANE backend from `/etc/sane.d`, and
  the scanner's interface was claimed by `usbfs` within 0.5 s of connecting. The helper held
  that claim until the scanner dropped off USB **exactly 90 s after connecting**, in all three
  observed runs (10:23:04 → 10:24:35, 10:27:01 → 10:28:32, 10:31:15 → 10:32:45). During that
  time `primefilm detect` correctly reported busy (exit 4). This also explains the
  "Device busy" and the unexplained disconnects recorded on 2026-11-08.
- **Fix**: a systemd override sets `SANE_CONFIG_DIR=/etc/colord-no-sane` (an empty `dll.conf`)
  for `colord.service`, so `colord-sane` loads no drivers. colord keeps managing display and
  printer profiles. Commands are in README.md, "Troubleshooting".
- **After the fix**: `colord-sane` did not run, the interface stayed unclaimed, and the
  scanner stayed on USB for the full 180 s observed (connected 10:35:58). `primefilm -v detect`
  succeeded 1 s after connect, in 1.07 s, and again at 180 s.
- **Detect output**: `pieusb:libusb:001:008  PIE MF Scanner (film scanner)`. The driver reports
  vendor `PIE` and model `MF Scanner`, not "Reflecta RPS 10M" as the `pieusb.conf` comment
  suggests. The simulated scanner (`FakeBackend`) and the README examples still use the
  Reflecta name; update them with the real recording (T040).
- Not yet checked: `capabilities` against the real scanner, whether opening it moves film (R2),
  unplug during a read (R3), Ctrl-C (quickstart steps 3–9, task T039).

## Decisions

### D1. Talk to SANE through hand-written FFI

- **Decision**: declare the 8 needed SANE functions and 3 structs ourselves in
  `src/backend/sane/ffi.rs`, linking with `#[link(name = "sane")]`.
- **Rationale**: the owner chose this after reviewing the alternatives. It needs no build
  script or libclang, every unsafe line is visible and commented (Principle VI), and it handles
  arrays, booleans and strings correctly.
- **Alternatives considered**:
  - `sane-scan` 0.1.2: last release March 2023; docs.rs never built it; panics when reading
    button or group options; reads SANE booleans as Rust `bool` (undefined behavior); truncates
    array options to the first value; requires libclang. Rejected.
  - `bindgen`: complete, but generated code is hard for a beginner to read and needs libclang.
    Rejected.

### D2. FFI surface and memory rules

Functions: `sane_init`, `sane_exit`, `sane_get_devices`, `sane_open`, `sane_close`,
`sane_get_option_descriptor`, `sane_control_option`, `sane_strstatus`.

ABI facts from `/usr/include/sane/sane.h`, mirrored with `#[repr(C)]`:

- `SANE_Word`, `SANE_Int`, `SANE_Bool`, `SANE_Fixed` are all 32-bit signed integers (`i32`).
  `SANE_Fixed` is 16.16 fixed point: divide by 65536.0 to get the real value.
- `SANE_Status`, `SANE_Value_Type`, `SANE_Unit`, `SANE_Constraint_Type` are C enums passed as
  `i32`. Values are kept as named `const`s and converted into Rust enums in the safe layer.
- `SANE_Device { name, vendor, model, type }`: four `const char*`.
- `SANE_Option_Descriptor { name, title, desc, type, unit, size, cap, constraint_type,
  constraint }`. `constraint` is a C union of three pointers, modeled as one
  `*const c_void` and cast according to `constraint_type`.
- `SANE_Range { min, max, quant }`: three words.
- Value reading uses only `SANE_ACTION_GET_VALUE` (0), so the tool cannot change anything
  (FR-006).
- Buffers: for Bool/Int/Fixed, a `Vec<i32>` of `size / 4` words (correct alignment, whole
  array read). For String, a `Vec<u8>` of `size` bytes, decoded with
  `CStr::from_bytes_until_nul` so a missing terminator cannot cause an over-read.
- Button and Group options have no value, and inactive options cannot be read; the tool records
  them without calling `sane_control_option`.
- Option 0 is the option count, per the SANE standard.
- `sane_get_devices` is called with `local_only = 1`; network scanners are out of scope.

### D3. Load only the pieusb driver

- **Decision**: at startup, write the repo's `sane.d/dll.conf` (just `pieusb`) and
  `sane.d/pieusb.conf` (embedded with `include_str!`) to `$XDG_RUNTIME_DIR/primefilm/sane.d`
  (falling back to the system temp dir), then set `SANE_CONFIG_DIR` to that path with no
  trailing colon so SANE ignores `/etc/sane.d`.
- **Rationale**: stops other backends claiming the scanner (the "Device busy" seen above) and
  keeps webcams such as the Cam Link out of the results (FR-002).
- **Rust note**: in edition 2024, `std::env::set_var` is `unsafe` because changing the
  environment while other threads read it is a data race. It is called in
  `backend/sane/config.rs` before the Ctrl-C handler starts its thread, with a `// SAFETY:`
  comment saying so.
- **Alternatives considered**: telling the user to export `SANE_CONFIG_DIR` (easy to forget);
  editing `/etc/sane.d/dll.conf` (needs root and affects VueScan users of other scanners).

### D4. Diagnose failures from SANE plus `/sys`

SANE alone cannot tell "off" from "busy": when pieusb cannot open the device during discovery
it just leaves it out of the list. The tool therefore reads `/sys/bus/usb/devices/*` first:

- `idVendor`/`idProduct` = `05e3`/`0144` → physically present; also read `busnum`, `devnum`.
- `<device>:1.0/driver` is a symlink to `usbfs` → another program holds the interface → busy.
- Opening `/dev/bus/usb/BBB/DDD` read-write fails with permission denied → permission denied.
  Opening the node does not claim the interface, so this check is harmless.
- `libsane-pieusb.so.1` absent from the SANE backend directory → driver unavailable.

Combined order and exit codes are defined in [contracts/cli.md](contracts/cli.md). The `/sys`
root path is a parameter so unit tests can point it at a fake directory tree.

### D5. Release the scanner on every path

- `Sane` calls `sane_exit` and `Device<'a>` calls `sane_close` in their `Drop` impls. Rust drops
  values in reverse order of creation, so the device always closes before SANE exits.
- A global `AtomicBool` makes a second `Sane::init` in the same process an error, because SANE
  allows only one active context.
- Ctrl-C: the `ctrlc` crate sets an `Arc<AtomicBool>`; the option-reading loop checks it
  between options and returns `Interrupted`, so the normal `Drop` path still runs.
- If the process is killed outright, Linux releases usbfs claims when the process exits, so the
  scanner is still freed for other programs (SC-004).

### D6. Crates

| Crate | Why | Alternative rejected |
|-------|-----|----------------------|
| `clap` 4 (derive) | Standard Rust CLI parser; subcommands, `--help`, usage errors (exit 2) for free | Parsing `std::env::args` by hand: more code, worse errors |
| `serde` + `serde_json` | Standard JSON in Rust; derive keeps output types and schema aligned | Writing JSON strings by hand: easy to produce invalid JSON (SC-005) |
| `thiserror` 2 | Derives `Display` and `Error` for our outcome enum; generated code is ordinary | Hand-written impls: repetitive boilerplate that teaches little |
| `ctrlc` 3 | Portable, small Ctrl-C hook with no `unsafe` in our code | Raw `libc::signal`: more unsafe code for no gain |

Not used: `anyhow` (we need typed outcomes, not opaque errors), `assert_cmd`/`tempfile` (the
standard library's `Command` and `env::temp_dir` suffice).

### D7. Testing without the scanner

- **FakeBackend** loads `src/backend/fixtures/primefilm-xa.json` and can simulate each outcome.
  The binary exposes it through a hidden `--simulate <scenario>` flag so integration tests
  exercise the real CLI, rendering and exit codes.
- **SANE `test` backend**: a hidden `--sane-backend test` flag writes a `dll.conf` containing
  `test` instead of `pieusb`. This runs the real FFI code against real C structures, covering
  integer, fixed, boolean, string, array, button and group options. Each integration test runs
  the binary as a separate process, so SANE's global state never clashes between tests.
- **Hardware tests** in `tests/hardware.rs` are `#[ignore]`d and run with
  `cargo test -- --ignored` when the scanner is on.

### D8. Summary of key capabilities

Derived from standard SANE option names: `resolution` (constraint values or range), `mode`
(string list), `depth` (list), `br-x` and `br-y` range maximum (scan area in mm). Infrared
support is reported as available when a mode contains "RGBI" or "infrared", or an option name
contains "infrared" (case-insensitive), and as unknown if neither is found. These names must be
checked against the real pieusb option list (R4).

## Open risks (verify on hardware)

- **R1, model byte**: the pieusb line `0x0048` was written for the PrimeFilm **XAs**. If the XA
  reports a different model in its inquiry data, pieusb will ignore it and the tool will say
  "driver unavailable". Check with `primefilm -vv detect` (sets `SANE_DEBUG_PIEUSB=5`) and
  adjust `sane.d/pieusb.conf`.
  *Resolved (2026-11-09)*: pieusb recognizes the XA with `0x0048`; `detect` lists it.
- **R2, side effects of opening**: confirm that `sane_open` on pieusb does not move film or
  start calibration. `scanimage -A` makes the same calls, so run `./scripts/probe.sh` first and
  watch the scanner.
- **R3, disconnects**: the disconnect path (exit 7) must be tested by unplugging the scanner
  during `capabilities`.
  *Update (2026-11-09)*: the earlier spontaneous disconnects were caused by colord (see
  findings above). The unplug test itself is still to do.
- **R4, option names**: the fixture starts hand-written from the pieusb option set, and the
  summary heuristics assume standard names. Replace the fixture with a real
  `primefilm capabilities --json` recording once hardware works, and update D8 if names differ.
  *Status (2026-11-08)*: `src/backend/fixtures/primefilm-xa.json` is **hand-authored** (12
  options in three groups, values invented to match the XA's published specs) and must be
  replaced with a real recording (tasks T040, quickstart step 10).

## Implementation notes (2026-11-08)

- SANE's `test` backend leaves its array, button and string test options inactive unless
  `enable-test-options` is set, so the repo ships `sane.d/test.conf` and `config::prepare`
  writes it alongside `dll.conf` when `--sane-backend test` is used.
- The `test` backend reports its `resolution` (a fixed-point option) as `0.000762939`:
  the raw word is the integer 50, not `SANE_FIX(50)`. `scanimage -A` shows the same value, so
  this is the test backend's quirk, not a conversion bug in our FFI.
- A value read returning `SANE_STATUS_UNSUPPORTED` or `SANE_STATUS_INVAL` (e.g. the test
  backend's hard-select-only bool) is recorded as `value: null` ("unreadable", per
  data-model.md); any other failing status aborts the read and is diagnosed (busy, access
  denied, disconnected, or SANE error).
