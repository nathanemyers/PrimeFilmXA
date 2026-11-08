# primefilm

A command-line tool for the **Pacific Image PrimeFilm XA** film scanner on Linux, built on the
open SANE `pieusb` driver. It is also a Rust learning project: the code is commented as a
teaching text (see [the constitution](.specify/memory/constitution.md), Principle VI).

Current features:

- `primefilm detect` checks that the scanner is connected and usable.
- `primefilm capabilities` lists every setting the scanner exposes, starting with a summary of
  the ones that matter for film (resolutions, color modes, bit depths, scan area, infrared).

The tool never scans, moves film or changes a scanner setting: it only reads.

## Prerequisites

- Rust stable via [rustup](https://rustup.rs) (`cargo --version` works)
- SANE runtime and headers: `sudo apt install libsane1 libsane-dev`
- The scanner on USB (it shows up as `05e3:0144` in `lsusb`), with VueScan closed

## Build

```bash
cargo build --release
```

The binary is `target/release/primefilm`. During development, `cargo run -q -- <args>` works too.

## Usage

```bash
primefilm detect
```

```text
Found 1 scanner:
  pieusb:libusb:001:004  Reflecta RPS 10M (film scanner)
```

```bash
primefilm capabilities
```

prints the scanner name, a `Summary` block, then every option grouped by section, with its
current value, allowed values and title. Inactive options are listed as `(inactive)`.

Options:

| Flag | Effect |
|------|--------|
| `--json` | Print one JSON document on stdout instead of text (schema in [output.schema.json](specs/001-scanner-detect/contracts/output.schema.json)) |
| `-v` | Show what the tool is doing on stderr |
| `-vv` | Also turn on the pieusb driver's own debug log |
| `--device <NAME>` | (`capabilities`) choose a scanner when more than one is connected |

```bash
primefilm --json capabilities | python3 -m json.tool
```

Failures print a message and a next step on stderr and exit with a distinct code (3 not found,
4 busy, 5 permission denied, 6 driver unavailable, 7 disconnected, 130 interrupted, 1 other
SANE error). The full table is in
[contracts/cli.md](specs/001-scanner-detect/contracts/cli.md#outcomes-and-exit-codes).

## Permissions

The tool runs as a normal user. If it reports "No permission to access /dev/bus/usb/…", add a
udev rule so your user can open the scanner:

```bash
echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="05e3", ATTR{idProduct}=="0144", MODE="0666"' | sudo tee /etc/udev/rules.d/60-primefilm.rules
```

```bash
sudo udevadm control --reload-rules
```

Then unplug and replug the scanner.

## Why only pieusb is loaded

With every SANE driver loaded, another driver can grab the scanner before pieusb does
("Device busy"), and webcams appear as scanners. On each run `primefilm` writes a private SANE
config to `$XDG_RUNTIME_DIR/primefilm/sane.d/` (from this repo's [sane.d/](sane.d/)) that loads
only pieusb. [scripts/probe.sh](scripts/probe.sh) does the same for `scanimage`, as an
independent cross-check.

## Tests

```bash
cargo test
```

runs everything that needs no scanner: unit tests, the CLI against a simulated scanner, and the
real SANE code against SANE's built-in `test` driver. With the scanner on:

```bash
cargo test -- --ignored
```

Before committing, all three must pass:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## How the code is organized

Read the modules in this order; each Rust concept is explained in a comment where it first
appears.

1. [src/lib.rs](src/lib.rs): the module tree, and `run`, which picks a backend and runs the
   command. Modules, `pub`, ownership and borrowing, generics.
2. [src/error.rs](src/error.rs): `ScanError`, every way a run can fail, with its message, next
   step and exit code. Enums, `match`, `Result` and `?`, unit tests.
3. [src/cli.rs](src/cli.rs): the command-line arguments, defined with clap. `#[derive]`.
4. [src/backend/mod.rs](src/backend/mod.rs): the `ScannerBackend` trait, the one interface all
   scanner access goes through, and `DeviceInfo`. Structs, traits.
5. [src/capability.rs](src/capability.rs): scanner options and the film summary. Iterators and
   closures, serde attributes.
6. [src/backend/fake.rs](src/backend/fake.rs): the simulated scanner used by tests, with
   [fixtures/primefilm-xa.json](src/backend/fixtures/primefilm-xa.json).
7. [src/report.rs](src/report.rs): text and JSON output. Serde.
8. [src/main.rs](src/main.rs): the binary: SANE config, Ctrl-C handling, exit codes. `Arc` and
   atomics.
9. [src/usb.rs](src/usb.rs): reads `/sys` to tell "off" from "busy" from "no permission".
10. [src/backend/sane/ffi.rs](src/backend/sane/ffi.rs): raw C declarations from `sane.h`. FFI,
    `#[repr(C)]`, raw pointers.
11. [src/backend/sane/config.rs](src/backend/sane/config.rs): writes the pieusb-only SANE config.
12. [src/backend/sane/mod.rs](src/backend/sane/mod.rs): the safe wrapper (`Sane`, `Device<'a>`)
    and `SaneBackend`. `unsafe` and `// SAFETY:`, `Drop`, the project's one lifetime.
13. [tests/](tests/): integration tests that run the real binary.

Design notes, decisions and open hardware questions are in
[specs/001-scanner-detect/](specs/001-scanner-detect/).
