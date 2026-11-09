# Hardware Validation: Black-and-White Negative Processing

**Feature**: [spec.md](spec.md) | **Experiments**: [research.md](research.md) |
**Steps**: [quickstart.md](quickstart.md#with-the-scanner)

Filled in during implementation. Record the date, film stock, flags under test and results.

## Experiments (E9–E11)

| # | Date | Film | Result | Notes |
|---|------|------|--------|-------|
| E9 | | | ⏳ Not run | RGB 16-bit vs RGBI 16-bit at 1800 dpi: times, R/G/B means, mean absolute difference. Until run, B&W scans capture RGBI (`BW_CAPTURE` in `src/scan.rs`). |
| E10 | | | ⏳ Not run | Raw 16-bit means of empty gate, clear base and a dense highlight at 1800 dpi; is the response linear (research P4)? |
| E11 | | | ⏳ Not run | `--keep-capture` fixtures: 3–4 frames each of HP5 and FP4 at `web` (dusty, blank/leader, high-key or night). Captures kept outside the repo. |

## Tuning (from E11)

| Constant | Before | After | Evidence |
|----------|--------|-------|----------|
| | | | |

## Validation steps (quickstart "With the scanner", 3–12)

| Step | Date | Result | Notes |
|------|------|--------|-------|
| 3 | | ⏳ Not run | Positive preview of HP5 in under 15 s |
| 4 | | ⏳ Not run | Single HP5 frame at `web`: `.jpg` looks like a print; no raw file |
| 5 | | ⏳ Not run | HP5 roll at `standard`: time per frame, folder size |
| 6 | | ⏳ Not run | FP4 roll at `standard` |
| 7 | | ⏳ Not run | Judging the results (SC-001, SC-006) |
| 8 | | ⏳ Not run | Rescan a wrong frame with `--brightness` / `--contrast` |
| 9 | | ⏳ Not run | `roll --match-tones` on a strip shot in one light |
| 10 | | ⏳ Not run | Interrupt a matched roll and resume with the printed command |
| 11 | | ⏳ Not run | `ls -a` the roll folder after each run (SC-007) |
| 12 | | ⏳ Not run | `cargo test -- --ignored` with an HP5 strip loaded |

## Success criteria

| Criterion | Target | Result |
|-----------|--------|--------|
| SC-001 | ≥ 9 of 10 frames look right with automatic settings | ⏳ |
| SC-002 | Time per frame at `standard` ≤ 109 s (002: 91 s + 20%) | ⏳ |
| SC-003 | Roll folder ≤ ⅓ of the raw size (≈ 143 MB per frame at `standard`) | ⏳ |
| SC-006 | ≥ 9 of 10 frames need no further dust spotting | ⏳ |
