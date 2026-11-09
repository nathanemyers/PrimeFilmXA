# Hardware Validation: Film Scanning

**Feature**: [spec.md](spec.md) | **Experiments**: [research.md](research.md) |
**Steps**: [quickstart.md](quickstart.md#with-the-scanner)

Filled in during implementation. Record date, film type, flags under test and results.

## Experiments (E1–E8)

| # | Date | Film | Result | Notes |
|---|------|------|--------|-------|
| E1 | 2026-11-09 | B&W negative strip, 4 frames | ✅ Pass | Flags `0x01` (slide transport, with command 0x17): `scanimage --mode Color --depth 8 --resolution 300 --advance=no` exit 0 in 19.5 s, no errors in the `SANE_DEBUG_PIEUSB=5` log, 431 × 287 image of a frame. `0x03` not needed. |
| E2 | 2026-11-09 | same | ✅ Pass | Second `--advance=no` scan: same frame (mean abs difference 1.7/255, noise level). `SLIDE INIT` before each scan does not move a loaded strip. |
| E3 | 2026-11-09 | same | ✅ Pass | Three `--advance=yes` scans: first was the same frame as E2 (diff 0.7), then frames 2 and 3 in order (diff 8–17 between neighbours). One frame per advance, correctly framed. |
| E4 | 2026-11-09 | same | ✅ Pass (finding) | Scanned frame 4, then 7 more scans with `--advance=yes`: all exit 0, no error or sense code in the log, and every scan was frame 4 again (diff 0.6–0.8/255). The owner confirmed the strip has 4 frames. **The XA silently stays on the last frame; there is no end-of-film status.** |
| E5 | 2026-11-09 | none (ejected) | ✅ Pass (finding) | Scan with no film: exit 0, no error. Image is the bare light source: channel means 205/199/206 of 255, std dev ~15, a few dark specks (dust). **No "no film" status either.** |
| E6 | 2026-11-09 | strip, frame 4 | ✅ Pass | `--preview=yes`: 431 × 287 (300 dpi) in 11.8 s (SC-002 < 15 s). No faster than a normal 300 dpi scan; about 6 s of every scan is calibration. Film did not move. |
| E7 | 2026-11-09 | strip, frame 4 | ✅ Pass | Color 8-bit: 1800 dpi → 2586 × 1721 in 28.9 s; 3600 → 5173 × 3443 in 50.0 s; 5000 → 7185 × 4783 in 54.1 s (one pixel more each way than `floor(mm / 25.4 × dpi)`; the driver rounds). Resolutions are honored; no snapping needed. |
| E8 | 2026-11-09 | strip, frame 1 (web) and 2 | ✅ Pass (finding) | `primefilm scan`, RGBI 16-bit: web 49 s (2586 × 1721, 35.6 MB); standard 91 s (5173 × 3443, 142.5 MB); high 314 s (7185 × 4783, 275 MB); max 985 s, **7185 × 9566** (550 MB). The sensor is 7185 pixels wide (pieusb `shading_width 7185`), i.e. 5000 dpi across the frame, although the scanner reports 10000 × 10000 dpi; at 10000 dpi only the lengthwise step gets finer, so the pixels are twice as tall as wide. The owner chose to drop `max`; `--resolution` is capped at 5000. SC-003 (standard ≤ half the time of the highest preset): 91 s vs 314 s ✅. Peak process memory grew with frame size (147 MB standard, 277 MB high): pieusb's memory-mapped `/tmp` buffer of the frame; our own buffer is 1 MiB. |

## Validation steps (quickstart "With the scanner", 1–9)

| Step | Date | Result | Notes |
|------|------|--------|-------|
| 2 | 2026-11-09 | ✅ Pass | `primefilm preview`: 431 × 287 RGB 8-bit in 11.8 s (SC-002). The first three attempts, right after the strip was inserted, failed with SANE "device busy" after ~10 s: the scanner was still loading the film. The tool reported this as "in use by another program" (exit 4), which is wrong while we hold the scanner; fixed to "the scanner is not ready (still loading the film?)" (exit 9). |
| 3 | 2026-11-09 | ✅ Pass | `primefilm scan --roll hw --preset web`: one file, 2586 × 1721 RGBI 16-bit, 35.6 MB, 49 s. Previews before and after differ by 2.1/255 (noise): film not moved (FR-014). Decoded the TIFF by hand: R, G, B and infrared channels all hold the image (silver blocks IR on B&W film). |
| 4 | 2026-11-09 | ✅ Pass | `primefilm advance`: 31 s, "Film advanced one frame."; the next preview shows frame 2 (difference 7.9/255 from the previous frame's preview). |
| 5 | 2026-11-09 | ✅ Pass | `primefilm roll --roll strip --preset web` on the reloaded 4-frame strip: frames 001–004 in film order (shelves, three people, couch, desk), about 51 s each; frame 005 matched frame 004 ("same as the previous frame; end of film"), was discarded, exit 0, `end-of-film`. Frame 004 recorded `film_moved: none`. |
| 6 | | ⏳ Not run | Ctrl-C and resume on hardware: the owner stopped the hardware session. Covered without hardware by `--simulate interrupt` (tests/scan_cli.rs). |
| 7 | 2026-11-09 | ✅ Pass | See E8. |
