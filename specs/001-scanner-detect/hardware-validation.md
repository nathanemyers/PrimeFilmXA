# Hardware Validation: Scanner Detection and Capability Listing

**Feature**: [spec.md](spec.md) | **Steps**: [quickstart.md](quickstart.md#hardware-validation-scanner-on)
| **Findings**: [research.md](research.md)

- **Date**: 2026-11-09
- **Scanner**: Pacific Image PrimeFilm XA, USB `05e3:0144`, reports itself as `PIE MF Scanner`
- **Host**: Ubuntu, SANE 1.2.1 (`libsane1`), Rust 1.99.1
- **Film loaded**: none (empty scanner)
- **Precondition**: colord prevented from loading SANE drivers (README.md, "Troubleshooting").
  Without it, colord claims the scanner on every connect, so `primefilm` reports busy
  (research.md, findings of 2026-11-09).
- **Known issue**: the scanner often drops off USB about 90 s after connecting; cause unknown
  (research.md, R3). It did so at 10:58:11, after step 6.

| Step | Check | Result | Notes |
|------|-------|--------|-------|
| 1 | Cross-check with `./scripts/probe.sh`; scanner must not move | ✅ Pass | All 39 non-group options match `scanimage -A` (name, value, inactive flag); no extras on either side. Nothing moved. |
| 2 | `detect` lists the scanner, exit 0, under 5 s (SC-001) | ✅ Pass | `pieusb:libusb:001:008  PIE MF Scanner (film scanner)`, 1.07 s, 1 s after connect. pieusb recognizes the XA with the shipped `pieusb.conf` (R1). |
| 3 | `capabilities` matches probe.sh (SC-002) | ✅ Pass | 43 options (39 + 4 groups) in 0.41 s; same values as step 1. Owner observed no film movement and no lamp/motor activity (R2). Kernel logs one USB reset per open; no disconnect. |
| 4 | `--json capabilities` is valid JSON (SC-005) | ✅ Pass | Parsed with Python's `json`; recorded as `src/backend/fixtures/primefilm-xa.json` (step 10). |
| 5 | Scanner released after each run (SC-004) | ✅ Pass | After every run, the next `primefilm` and `scanimage` opened it; 10 back-to-back concurrent runs below all ended with the scanner free and on USB. |
| 6 | Busy: VueScan holds the scanner → exit 4 | ✅ Pass | With VueScan open on the scanner (10:57): `/sys` showed the interface claimed, `detect` printed the busy message and VueScan next step, exit 4 in 3 ms without starting SANE; `--json` gave `"outcome": "busy"`, exit 4. Busy from colord and from a second `primefilm` also gave exit 4. After VueScan closed (10:57:40), `detect` found the scanner unclaimed, exit 0. While opening it, VueScan made the scanner disconnect and re-enumerate (device 8 gone at 10:56:38, device 9 at 10:56:42). |
| 7 | Unplug during `capabilities` → exit 7, no retry (R3) | ⏭ Skipped | Skipped by the owner. Covered without hardware by `--simulate disconnect` (exit 7). |
| 8 | Ctrl-C during `capabilities` → exit 130, then step 5 passes | ⏭ Skipped | Skipped by the owner. Covered without hardware by `--simulate interrupt` (exit 130). |
| 9 | `cargo test -- --ignored` | ✅ Pass | After the fix below: 2/2 passed in 5 of 5 runs. |
| 10 | Record the fixture | ✅ Done | Real 43-option recording (T040); D8 summary heuristics unchanged. |

## Problems found and fixed

- **Parallel hardware tests collided.** `cargo test` runs tests on several threads, so both
  hardware tests opened the one scanner at once and one failed. `tests/hardware.rs` now holds
  a `Mutex` while running `primefilm`. Before: 1 of 2 failed, alternating. After: 5/5 clean.
- **A second program grabbing the scanner was reported as "driver unavailable" (exit 6).** The
  `/sys` busy check runs before SANE starts; if another process claims the scanner just
  after it, pieusb silently skips the device and the empty list looked like a driver problem.
  `SaneBackend` now re-checks the claim when pieusb lists nothing while the scanner is on USB.
  Verified with 10 runs of `capabilities` and `detect` started together: every run gave one
  exit 0 and one exit 4, never 6.
