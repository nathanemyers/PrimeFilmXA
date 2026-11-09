# Research: Black-and-White Negative Processing

**Feature**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md) | **Date**: 2026-11-09

Builds on feature 002's research ([../002-film-scan/research.md](../002-film-scan/research.md)),
whose decisions D1–D12 still hold unless replaced here. Decisions in this feature are numbered
P1–P14 to keep them apart from 002's.

## Starting point (what feature 002 does today)

- Full scans are RGBI 16-bit, streamed from SANE in 1 MiB chunks straight into our TIFF writer
  (002 D3); the process never holds a whole frame. pieusb itself keeps the frame in a
  memory-mapped `/tmp` buffer (peak 147 MB at `standard`, 277 MB at `high`, 002 E8).
- Time per frame (002 E8): `web` 49 s, `standard` 91 s, `high` 314 s.
- On B&W film the infrared channel holds the full silver image, not just dust (002 hardware
  validation, test 3).
- A capture already feeds `FrameStats` (empty-gate and repeated-frame checks, 002 D4). Those
  checks stay exactly as they are.

## Hardware experiments (run before the code that depends on them)

Continuing 002's numbering. Results go in `hardware-validation.md` for this feature.

| # | Question | Method | Decides |
|---|----------|--------|---------|
| E9 | Is an RGB-only 16-bit capture faster than RGBI, and identical in its R, G, B data? | `scanimage --mode Color --depth 16` vs `--mode RGBI` at `web`, same frame, film not moved; time both, compare R/G/B means and a diff. | Capture format for B&W (P3). |
| E10 | Are the 16-bit samples linear in light? | Scan the empty gate and the film base at `web`; compare against pieusb's reported exposure/gain; check that a step of known density (frame gap vs base vs a dense highlight) gives raw ratios consistent with `10^-D`. A rough check is enough. | The density model (P4). |
| E11 | Real fixtures for tuning | With the hidden `--keep-capture` flag (P12), scan 3–4 frames each of HP5 and FP4 at `web`, including a dusty frame, a blank frame and a high-key frame. Keep the raw captures outside the repo; commit one cropped `web` capture per stock as a test fixture if it stays under 2 MB. | Profile constants (P6), dust thresholds (P8), SC-001/SC-006 checks. |

E9 and E10 do not block the code: the design works with either capture format and with a
moderately non-linear response (P4). E11 is needed before tuning the constants.

## Decisions

### P1. Processing runs inside the capture, in memory, one frame at a time

- **Decision**: while a frame streams in, each chunk updates `FrameStats` (unchanged) and a new
  `GrayFrame` accumulator that keeps one 16-bit gray value per pixel in a `Vec<u16>`. When the
  frame ends, the processing pipeline (P4–P9) runs on that buffer and the outputs are written.
  No raw data reaches the disk (spec FR-014).
- **Memory**: 2 bytes per pixel: 36 MB at `standard`, 69 MB at `high`, plus a 1-byte-per-pixel
  dust mask while dust removal runs (P8): at most about 105 MB at `high`, alongside pieusb's own
  277 MB buffer. Bounded per frame and never grows with roll length.
- **Rationale**: automatic black and white points need statistics of the whole frame before
  any output pixel is known, and dust repair needs neighbouring rows. Holding only gray (not
  RGBI) cuts the buffer to a quarter of the raw frame.
- **Alternatives rejected**: (a) spill the raw frame to a temporary file and process it in two
  streaming passes: bounded at 1 MiB, but writes 143–275 MB of raw data to disk per frame, which
  the owner chose not to keep, and needs crash cleanup on every path; (b) keep RGBI in memory:
  four times the memory for no gain on B&W film.

### P2. Gray from the color channels: plain average of R, G and B

- **Decision**: gray = (R + G + B) / 3 per pixel, in integer arithmetic on the raw 16-bit
  values. The infrared channel, if present, is skipped.
- **Rationale**: silver is spectrally neutral, so all three channels hold the same image; equal
  weights average out sensor noise best. The scanner's color cast and any film-base tint simply
  vanish in a gray image (spec FR-003: no tint is possible by construction).
- **Alternatives rejected**: luminance weights (0.2126/0.7152/0.0722) are meant for color
  images and would throw away most of the blue channel's signal; per-channel normalisation
  first gains nothing once the channels are merged.

### P3. Capture format for B&W: RGB 16-bit if E9 confirms it, otherwise RGBI 16-bit

- **Decision**: add `ColorFormat::Rgb16` to the backend. B&W scans use it if E9 shows it is
  faster and gives the same R, G, B data; otherwise they keep `Rgbi16` and P2 ignores the
  infrared samples. Previews stay `Rgb8`.
- **Rationale**: infrared is useless on silver film (002 test 3); skipping it may shorten each
  scan. The pipeline handles 3 or 4 channels the same way, so the choice is a one-line setting.

### P4. Tones are set in density, through one lookup table

- **Decision**: every tone step is a function of the gray value alone, so the whole tone
  mapping is one 65,536-entry table from raw gray to output 16-bit. Per frame:
  1. **Density**: `d(v) = log10(65535 / max(v, 1))`: 0 for full-scale light, about 2 for the
     densest silver. Film density is close to linear in the log of the exposure, so equal
     steps in density are close to equal perceived steps in the print.
  2. **Black and white points** (P5): `d_black` (film base and shadows) and `d_white`
     (densest highlights), automatic or explicit.
  3. **Normalise**: `t = clamp((d − d_black) / (d_white − d_black), 0, 1)`. The negative's
     clear base becomes black and its densest parts white (spec FR-002).
  4. **Contrast** `c` in −50…+50 (P7): `t = tᵃ / (tᵃ + (1 − t)ᵃ)` with `a = 2^(c / 50)`, an
     S-curve that steepens or flattens the midtones and keeps 0 and 1 fixed.
  5. **Brightness** `b` in −50…+50: `t = t^g` with `g = 2^(−b / 50)`; positive values lift the
     midtones, black and white stay fixed.
  6. **Output**: `round(t × 65535)`. Output values are display-referred gray (treated by
     viewers like sRGB gray); this is recorded in the settings.
- **Rationale**: a table makes the per-pixel work one array lookup (fast, and identical on
  every run: spec FR-007), and the maths reads like the darkroom: density in, paper grade
  (contrast) and print exposure (brightness) out. Mapping density linearly to encoded output
  gives a natural, slightly flat-but-full rendering the owner can steepen with `--contrast`.
- **If E10 shows the response is not linear**: the auto points still anchor black and white,
  so only the midtone curve differs; it can be corrected with a fixed pre-curve in the table
  without changing anything else.
- **Alternatives rejected**: simple `65535 − v` inversion with auto-levels, which squashes the
  shadows of the print into a few values because transmitted light is exponential in density.

### P5. Automatic black and white points from a histogram of the picture area

- **Decision**: build a 65,536-bin histogram of gray values over the frame minus a margin of
  2 mm on each side (frame gap and film edge, spec edge case; about 140 px at `standard`).
  `d_black` is the density at the 0.1th percentile of density (lightest-on-film end) and
  `d_white` at the 99.95th percentile, read from the cumulative histogram. Both fractions are
  profile constants (P6).
- **Opaque pixels are not silver**: pixels denser than `d_black + opaque_range` (2.0 D to
  start, a profile constant) are left out of the white point. A normally developed B&W
  negative's highlights stay well below that; opaque dust does not. Without this, a dusty frame
  (dust over 0.05% of the area) would set its white point on the dust. *(Added during
  implementation.)*
- **Rationale**: percentile clipping keeps a few specular highlights or dust specks from
  deciding the white point alone (spec FR-004). A histogram over 65,536 bins is exact and costs
  one pass.
- **Probably blank**: if `d_white − d_black` is below the profile's `min_range` (0.15 D to start,
  tuned in E11), the frame is flagged "probably blank" and the mapping uses
  `d_white = d_black + nominal_range` (1.2 D to start) instead, so grain is not stretched into a
  full-contrast image. The flag goes into progress, the summary, JSON and the settings record.

### P6. Film types, stocks and profiles

- **Decision**: `enum FilmType { BwNegative }` and `enum FilmStock { Hp5, Fp4, Bw }`, where each
  stock knows its type and returns a `Profile { black_clip, white_clip, min_range,
  nominal_range, contrast, brightness, dust }`. HP5, FP4 and generic B&W return identical
  profiles for now, as separate `match` arms so each can be tuned alone (spec FR-012). `--film`
  takes `hp5`, `fp4` or `bw` (default `bw`). `--film color` is accepted by the parser only to
  fail with "Color film is not supported yet." (usage error, exit 2) before the scanner opens
  (spec FR-013). `primefilm films` lists the stocks (spec FR-011).
- **Rationale**: plain enums with `match` keep everything visible to the owner (Principle VI);
  adding `ColorNegative` later means a new variant whose missing `match` arms the compiler
  points out.
- **Alternatives rejected**: a `trait FilmProcessor` with one implementation per type: a trait
  object or generics for a single implementation today is speculative (Principle V).

### P7. Owner adjustments and tone matching

- **Decision**:
  - `--brightness N` and `--contrast N`, integers −50…+50 (default 0, from the profile).
  - `--black D --white D`, densities 0.00–4.00 with `black < white`, given together: explicit
    points instead of automatic ones.
  - `roll --match-tones`: the first frame that is not probably blank sets the points; every
    later frame of the run reuses them. Frames before that first good frame keep their own
    points. Each settings record says where its points came from: `auto`, `explicit`, or
    `matched` with the frame number.
  - When a matched roll stops early, the resume command printed in the summary includes
    `--black` and `--white` with the locked values, so the rest of the roll matches (spec US5
    scenario 3).
  - All values are checked by clap or before the scanner opens (spec FR-020).
- **Rationale**: the first good frame is known as soon as it is scanned, so every frame can
  still be finished and saved as it is captured; whole-roll statistics would need every raw
  frame kept until the end (spec assumption).

### P8. Dust removal: opaque specks are denser than any silver on the frame

- **Decision**: dust removal runs on the gray buffer after the points are known and before the
  tone table is applied:
  1. **Candidates**: pixels whose density exceeds `d_silver + margin`, where `d_silver` is the
     densest pixel in the picture-area histogram that is not opaque (P5): the densest silver on
     the frame. Dust is opaque, so almost no light passes it; stars and catchlights are silver,
     so they are never denser than `d_silver`. *(Changed during implementation: the plan
     compared against the clipped white point `d_white`, but on a night sky the stars are under
     0.05% of the frame, so `d_white` falls below their cores and they were erased. The
     synthetic star-field test caught it.)*
  2. **Components**: group candidates into 8-connected components with a flood fill (explicit
     stack, no recursion). Keep components whose area is at most `max_area` and whose
     bounding box fits in `max_span` (hairs are long but thin). Larger components are left
     alone (spec edge case: long scratches only partly removed).
  3. **Mask**: grow each kept component by `dilate` pixels to cover its soft edge.
  4. **Repair**: fill each masked pixel with the mean of the unmasked pixels in the smallest
     square window around it (radius 2, 4, 8, … up to 32 px) that holds at least 8 of them.
     Pixels outside the mask are never changed (spec FR-008).
  5. **Points again**: recompute the histogram and auto points after repair, so a large speck
     cannot have nudged the white point.
- **Strength** (spec FR-009): `--dust off|gentle|normal|strong`, default `normal`:

  | Level | `margin` (D) | `max_area` | `max_span` | `dilate` |
  |-------|--------------|------------|------------|----------|
  | gentle | 0.50 | 0.05 mm² | 1.0 mm | 1 px |
  | normal | 0.30 | 0.10 mm² | 2.0 mm | 2 px |
  | strong | 0.15 | 0.25 mm² | 4.0 mm | 3 px |

  Areas and spans are converted to pixels from the frame's resolution, so behaviour is the
  same at every preset. Starting values, to be tuned on E11 fixtures.
- **Recorded**: level, the constants used, and the number of components repaired.
- **Rationale**: the physical difference between dust and image (opaque vs. bounded silver
  density) is a single, explainable rule the owner can read and tune, rather than a statistical
  detector. It keeps small bright image details by construction (SC-005).
- **Known risk**: light scattering around very small specks makes them less dense than truly
  opaque, so the smallest specks may be missed at `gentle`. E11 measures this; `strong` exists
  for dusty rolls.
- **Alternatives rejected**: infrared dust detection (does not work on silver film, spec);
  median-filter difference detection (also flags grain and stars, and needs a full-frame
  median, which is slow); learned inpainting (a large dependency the owner cannot read).

### P9. Pipeline order per frame

1. Stream chunks → `FrameStats` + `GrayFrame` (P1, P2). Empty gate / repeated frame checks run
   as in 002, before any processing.
2. Histogram of the picture area → automatic points (P5), unless explicit or matched (P7).
3. Dust removal if on (P8), then points again if automatic.
4. Build the tone table (P4) and map the buffer in place to the 16-bit positive.
5. Write the share copy (P10) from the same buffer, then publish.

### P10. Outputs: 16-bit gray TIFF master, 8-bit JPEG share copy, JSON record

- **Decision**:
  - **Master**: `<roll>-NNN.tif`, 1 sample per pixel, 16-bit, uncompressed,
    `PhotometricInterpretation = BlackIsZero`. Our TIFF writer (002 D3) gains 1-channel support.
  - **Share copy**: `<roll>-NNN.jpg`, full resolution, 8-bit gray, JPEG quality 90, made by
    rounding the master's values to 8 bits. About 3–5 MB at `standard`.
  - **Record**: `<roll>-NNN.json`, version 2 (data-model.md).
  - Publishing order: record, share copy, then master, each by hard link from `.partial` (002
    D7). The master appears last, so a `.tif` always means a complete frame (spec FR-018).
    The free-name check (`-2`, `-3`, …) now also checks `.jpg`.
- **Disk at `standard`**: about 36 MB + 4 MB per frame instead of 143 MB: 28% of the raw size
  (SC-003).
- **Rationale**: TIFF keeps the master readable by every archival tool with our existing,
  readable writer. JPEG is what every phone, browser and sharing site accepts.
- **Alternatives rejected**: PNG for the share copy (lossless, but ~3× larger and not what
  sharing sites expect); a separate `share/` folder (file extension already separates them and
  keeps each frame's files together).

### P11. Previews are processed too, without dust removal

- **Decision**: previews (8-bit RGB capture, 002 D9) go through P2, P4–P5 with the chosen stock
  and adjustments, and are saved as an 8-bit gray TIFF plus record in `previews/`. Dust removal
  is skipped to keep previews fast and because they are not archival.
- **Rationale**: spec US6; framing and exposure are judged on a positive.

### P12. Development-only: keep a raw capture and replay it

- **Decision**: a hidden `--keep-capture DIR` flag (like 002's hidden `--simulate`) also streams
  the raw capture into a TIFF in `DIR`, using the 002 path unchanged. An example program,
  `examples/process_capture.rs`, runs the processing pipeline on such a TIFF with any settings
  (decoded with the `tiff` dev-dependency) and writes the outputs, so profiles and dust
  thresholds can be tuned without rescanning.
- **Rationale**: tuning (E11, SC-001, SC-006) needs the same frame processed many ways. The
  constitution allows deliberately kept captures as test fixtures; the flag is hidden and never
  used in normal scanning, so normal output keeps no raws (spec FR-014).

### P13. New crate: `jpeg-encoder` 0.6

| Crate | Why | Alternative rejected |
|-------|-----|----------------------|
| `jpeg-encoder` 0.6 | Pure-Rust JPEG encoder with grayscale support and no dependencies; the encoder used inside the widely used `image` crate. JPEG's DCT and Huffman coding are file-format plumbing, not the processing this tool exists for. | Writing our own JPEG encoder (about 1,000 lines unrelated to film); the `image` crate (many transitive dependencies for one encoder); shelling out to `cjpeg` (a runtime dependency outside Cargo). |

No crate for image processing: histogram, tone table, flood fill and repair are our own code
(Principle VI).

### P14. Testing without the scanner

- **Synthetic negatives** built in tests: a 21-step density wedge (SC-004), a frame with 50
  opaque specks and hairs on a grainy mid-tone (SC-005), a star field of small dense silver
  blobs (SC-005), a blank frame (P5), frames with a clear 2 mm border (P5 margin).
- **Determinism** (spec FR-007): process the same buffer twice and
  compare bytes.
- **Fake backend**: its generated frames become negative-like (gray pattern with a film-base
  border and three opaque one-pixel specks; the simulated frames are 1/50 of real size, where
  even the smallest dust limit is one pixel) so CLI tests produce positives and exercise dust
  removal.
  Film-order checks move from the saved image's first sample (002) to unit tests on the
  captured gray frame, since the positive no longer carries the raw sample.
- **CLI tests**: files per frame (`.tif`, `.jpg`, `.json`, no raw), usage errors for `--film
  color`, ranges and `--black` without `--white`, `films` listing, JSON against schema v2,
  resume command with `--black/--white` after a matched roll is interrupted, no `.partial` or
  raw file after jam, disconnect and interrupt runs (SC-007).
- **Outputs checked independently**: master decoded with the `tiff` crate; JPEG checked for
  its SOI/EOI markers and dimensions in its SOF header (no JPEG decoder dependency).
- **Hardware** (`#[ignore]`): scan one frame and a short strip of HP5 and FP4; timing against
  002 E8 (SC-002).

## Open risks

- **R1, dust density margin**: if specks on real scans are much less opaque than assumed, the
  margins in P8 need lowering, which raises the chance of catching the brightest real
  highlights on overexposed negatives. Mitigation: E11 fixtures, the `gentle` level, and `off`.
- **R2, linearity**: see P4; worst case is a fixed pre-curve.
- **R3, processing time at `high`**: 34 M pixels; every pass is linear in pixels and the dust
  search only floods candidates. Estimated a few seconds per frame against 314 s of scanning;
  measured in the hardware tests.
- **R4, color film**: until the color feature exists, the tool cannot scan color film at all
  (spec assumption). Owner accepted.
