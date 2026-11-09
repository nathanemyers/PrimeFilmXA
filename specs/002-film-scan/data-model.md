# Data Model: Film Scanning

**Feature**: [spec.md](spec.md) | **Research**: [research.md](research.md)

Entities from feature 001 (`DeviceInfo`, `Capability`, `ScanError`) are reused unchanged
except where noted. Rust type names are given for orientation; the plan places them in modules.

## Preset (`Preset`, enum)

A named full-scan quality level (FR-005). Fixed set; not configurable.

| Name | Resolution (dpi) | Pixels (full frame) | RGBI 16-bit file | Per 36 frames |
|------|------------------|---------------------|------------------|---------------|
| `web` | 1800 | 2586 × 1721 | ≈ 36 MB | ≈ 1.3 GB |
| `standard` (default) | 3600 | 5173 × 3443 | ≈ 143 MB | ≈ 5.1 GB |
| `high` | 5000 | 7184 × 4782 | ≈ 275 MB | ≈ 9.9 GB |

- Pixel sizes are computed from the scanner's scan area (36.50 × 24.30 mm, feature 001
  recording) as `floor(mm / 25.4 × dpi)`; the scanner's real output size is recorded per scan.
- Time per frame shown by `primefilm presets` was measured in hardware experiment E8: web
  49 s, standard 91 s, high 5 min 14 s.
- There is no 10000 dpi preset: the scanner's sensor resolves at most 5000 dpi across the
  frame, and above that it only adds lines lengthwise, giving stretched images (E8).
- Every preset scans in mode `RGBI` at 16 bits with `clean-image` and `correct-infrared` off
  (research, findings). Presets differ only in resolution (spec clarifications).
- `--resolution DPI` (FR-006) replaces the preset's resolution; it must be within the scanner's
  range (25–5000) and the recorded preset becomes `custom`.

## Scan Settings (`ScanSettings`)

What one capture asks of the scanner. Built from the command; turned into scanner options by
the backend.

| Field | Type | Notes |
|-------|------|-------|
| `kind` | `Full` \| `Preview` \| `Throwaway` | `Throwaway` is the advance-only scan (research D5) |
| `resolution_dpi` | number | Ignored by pieusb for `Preview` (it uses its preview resolution) |
| `color` | `Rgbi16` \| `Rgb8` | `Full` → `Rgbi16`; `Preview` and `Throwaway` → `Rgb8` |
| `advance_after` | bool | True only for roll frames and `Throwaway` |

pieusb option mapping (`Full`): `mode=RGBI`, `depth=16`, `resolution=<dpi>`, `preview=false`,
`advance=<advance_after>`, `clean-image=false`, `correct-infrared=false`, scan area left at the
full frame. `Preview`: `mode=Color`, `depth=8`, `preview=true`, `advance=false`.
`Throwaway`: `mode=Color`, `depth=8`, `resolution=25`, `advance=true`.

## Frame Layout (`FrameParams`)

Returned when a frame starts (from `sane_get_parameters`).

| Field | Rules |
|-------|-------|
| `pixels_per_line`, `lines` | > 0, else the frame is a scan failure |
| `bits_per_sample` | 8 or 16; anything else is a scan failure |
| `channels` | `bytes_per_line / (pixels_per_line × bits_per_sample / 8)`; must be 3 or 4 |
| `bytes_per_line` | must equal `pixels_per_line × channels × bytes_per_sample` exactly |

## Frame Statistics (`FrameStats`)

Collected while a frame streams to disk (research D4):

- per color channel (R, G, B; infrared ignored), the fraction of samples ≥ `NEAR_FULL` (50% of
  full scale); `looks_empty()` is true when every color channel's fraction ≥ `EMPTY_FRACTION`
  (95%);
- a **fingerprint**: the average of each color channel in each cell of a 16 × 16 grid, as a
  fraction of full scale (768 numbers). `same_frame(a, b)` is true when the mean absolute
  difference of two fingerprints is below `SAME_FRAME_DIFF` (0.01).

Thresholds come from hardware experiments E4/E5 (research, findings).

## Roll (`Roll`)

| Field | Rules |
|-------|-------|
| `name` | 1–64 characters from `A–Z a–z 0–9 - _ .`, not starting with `.`; default: today's local date `YYYY-MM-DD` |
| `folder` | `<output root>/<name>/`, created if missing |
| `next_frame` | 1 + the highest `NNN` among files named `<name>-NNN*.tif` in `folder`; 1 if none |

## Saved Scan (`SavedScan`)

One archived image and its settings record (FR-007 to FR-011).

| Field | Notes |
|-------|-------|
| `frame` | 1–999 |
| `tiff_path` | `<roll>-<NNN>.tif`, or `<roll>-<NNN>-<k>.tif` (k = 2, 3, …) if taken |
| `sidecar_path` | same stem, `.json` |
| `seconds` | capture plus write time |
| `film_moved` | `advanced` \| `none` |

Sidecar JSON contents are listed in research D10. File publication order and the no-overwrite
rule are in research D7.

## Preview (`SavedPreview`)

`<output root>/previews/preview-<YYYYMMDD-HHMMSS>.tif`, 8-bit RGB, with the same sidecar
fields minus roll and frame. Never written inside a roll folder.

## Run Outcome

The result of a `scan`, `roll`, `preview` or `advance` run: the list of files saved plus one
outcome. Feature 001's `ScanError` gains three variants (exit codes in
[contracts/cli.md](contracts/cli.md)):

| Outcome | Meaning |
|---------|---------|
| `completed` | Single scan, preview or advance finished |
| `end-of-film` | Roll scan reached the end of the film, or the `--frames` limit (success); `advance` when the film was already on its last frame |
| `no-film` (new) | Nothing to scan on the first frame |
| `scan-failed` (new) | The scanner reported an error during a frame (jam, invalid settings, I/O) while still on USB |
| `output-error` (new) | A file or folder could not be written (disk full, permissions, bad path) |
| `interrupted` | Ctrl-C; the frame in progress was finished and saved first (research D6) |
| `disconnected`, `busy`, `not-found`, … | As in feature 001 |

### Roll scan states

```text
            ┌──────────────────────────────────────────────┐
            ▼                                              │ frame saved, film advanced,
 [start] → capture frame N ──(saved)──▶ check stop ────────┘ no stop requested
              │                            │
              │ same as previous frame,    ├─ --frames reached ────────▶ end-of-film (exit 0)
              │ or empty gate (N > first)  ├─ Ctrl-C requested ────────▶ interrupted (130)
              ▼
         end-of-film (exit 0)          on N = first: empty gate ▶ no-film (8)
         scanner error ▶ scan-failed (9)   disconnect ▶ disconnected (7)   write error ▶ output-error (10)
```

In every failure state, the frame in progress is discarded (only its `.partial` file existed)
and all earlier frames stay saved. The summary names the last frame saved and the
`--start-frame` to resume with.
