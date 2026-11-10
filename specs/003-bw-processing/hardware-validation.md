# Hardware Validation: Black-and-White Negative Processing

**Feature**: [spec.md](spec.md) | **Experiments**: [research.md](research.md) |
**Steps**: [quickstart.md](quickstart.md#with-the-scanner)

Filled in during implementation. Record the date, film stock, flags under test and results.

## Findings

| Date | Film | Result | Notes |
|------|------|--------|-------|
| 2026-11-09 | HP5 (owner's first frames) | ❌ Artifacts | Tone-balanced positives with dust removal at `normal` showed outlines, flat patches and clipped highlights on some frames. Tone balancing removed and dust removal turned off by default (research P15). |
| 2026-11-09 | HP5, checked in VueScan | ⚠️ Film | The same frames show the same outlines and flat patches in VueScan: the artifacts are on the negatives (development), not from this tool (research P17). |
| 2026-11-09 | HP5, `roll --preview` | ❌ Flat | Fixed linear and density inversions looked flat and grey: the 8-bit raw negative used only values 2–52 of 255. Frame 003 re-processed with the automatic points and dust off: black 0.888 D, white 2.106 D, clean, no outlines. Automatic points restored, dust removal stays off (research P16). |

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

## Validation steps (quickstart "With the scanner", 3–10)

| Step | Date | Result | Notes |
|------|------|--------|-------|
| 3 | | ⏳ Not run | Positive preview of HP5 in under 15 s |
| 4 | | ⏳ Not run | Single HP5 frame at `web`: positive free of outlines and flat patches; no raw file |
| 5 | | ⏳ Not run | HP5 roll at `standard`: time per frame, folder size |
| 6 | | ⏳ Not run | FP4 roll at `standard` |
| 7 | | ⏳ Not run | Judging the results (SC-001) |
| 8 | | ⏳ Not run | Dust levels on a dusty frame vs `--dust off` (SC-006) |
| 9 | | ⏳ Not run | `ls -a` the roll folder after each run (SC-007) |
| 10 | | ⏳ Not run | `cargo test -- --ignored` with an HP5 strip loaded |
| US7 | | ⏳ Not run | `roll --preview` on a 4–6 frame strip: previews in film order, ~12 s per frame, ends at the end of the strip (T069) |

## Success criteria

| Criterion | Target | Result |
|-----------|--------|--------|
| SC-001 | No artifacts beyond what VueScan shows for the same frame; 9 of 10 frames need only ordinary finishing | ⏳ |
| SC-002 | Time per frame at `standard` ≤ 109 s (002: 91 s + 20%) | ⏳ |
| SC-003 | Roll folder ≤ ⅓ of the raw size (≈ 143 MB per frame at `standard`) | ⏳ |
| SC-006 | Tuned dust removal leaves no visible artifacts on real frames | ⏳ |
