# Quickstart: Scanner Detection and Capability Listing

How to build `primefilm` and prove the feature works. Commands and exit codes are defined in
[contracts/cli.md](contracts/cli.md); output fields in [data-model.md](data-model.md).

## Prerequisites

- Rust stable via rustup (`cargo --version` works)
- `libsane1` and `libsane-dev` installed (`ls /usr/include/sane/sane.h`)
- For hardware checks: the PrimeFilm XA powered on, VueScan closed

## Build and check (no scanner needed)

```bash
cargo build
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Expected: all pass with the scanner off. `cargo test` covers every outcome through the fake
backend and the real FFI code through SANE's `test` backend.

## Simulated runs (no scanner needed)

| Command | Expected |
|---------|----------|
| `cargo run -q -- --simulate ready detect` | Lists one simulated scanner; exit 0 |
| `cargo run -q -- --simulate ready capabilities` | Summary, then grouped option list |
| `cargo run -q -- --simulate ready --json capabilities \| python3 -m json.tool` | Valid JSON |
| `cargo run -q -- --simulate busy detect; echo $?` | Busy message on stderr; `4` |
| `cargo run -q -- --simulate no-scanner detect; echo $?` | Not-found message; `3` |
| `cargo run -q -- --simulate disconnect capabilities; echo $?` | Disconnect message; `7` |
| `cargo run -q -- --sane-backend test capabilities` | Options from SANE's test device, including arrays, buttons and groups |

## Hardware validation (scanner on)

1. **Cross-check with SANE directly**: `./scripts/probe.sh`. Watch the scanner: it should not
   move film (research R2).
2. **Detect** (US1, SC-001): `time cargo run -q -- detect`. Expect the scanner listed, exit 0,
   under 5 seconds. If it says "driver unavailable", run `cargo run -q -- -vv detect` and
   compare the model number with `sane.d/pieusb.conf` (research R1).
3. **Capabilities** (US2, SC-002): `cargo run -q -- capabilities`, then compare each option
   against `./scripts/probe.sh`. Every option must appear with the same value.
4. **JSON** (SC-005): `cargo run -q -- --json capabilities | python3 -m json.tool`.
5. **Release** (SC-004): immediately after each run, `scanimage -L` (or VueScan) must be able to
   open the scanner.
6. **Busy** (US3): open VueScan with the scanner, then `cargo run -q -- detect; echo $?` → `4`.
7. **Disconnect** (US3, research R3): start `cargo run -q -- capabilities` and unplug the USB
   cable mid-run → `7`, no retry loop.
8. **Ctrl-C**: press Ctrl-C during `capabilities` → `130`, then step 5 passes.
9. **Hardware tests**: `cargo test -- --ignored`.
10. **Record the fixture** (research R4): once step 3 matches,
    `cargo run -q -- --json capabilities > src/backend/fixtures/primefilm-xa.json` (keep the
    `options` part as the fixture format requires), and update research D8 if option names
    differ.

Record the date, film loaded and results in this feature folder (constitution, Development
Workflow).
