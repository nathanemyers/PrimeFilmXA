# Data Model: Black-and-White Negative Processing

**Feature**: [spec.md](spec.md) | **Research**: [research.md](research.md)

Extends feature 002's model ([../002-film-scan/data-model.md](../002-film-scan/data-model.md)).
Presets, roll folders, frame numbering and run outcomes are unchanged unless noted.

> **Revision 2026-11-09 (research P16), superseding part of P15 below:** Tone Points are back
> (sources `auto` and `blank-default` only) with the profile's `white_clip`, `min_range` and
> `nominal_range`; the record's `processing` again has `points` and `probably_blank`. Each
> frame is one `<stem>.tif`; the two-inversion comparison is gone (`output.inversion` removed).
>
> **Revision 2026-11-09 (research P15).** Withdrawn: Tone Points, the tone-matching state, the
> blank-frame flag, brightness/contrast/explicit points in Processing Settings, the profile's
> `white_clip`, `min_range` and `nominal_range`, and the share copy. Processing Settings are
> now just `stock` and `dust` (default `off`). Each frame is saved as two positives,
> `<stem>.tif` (inversion `linear`) and `<stem>-density.tif` (inversion `density`), and the
> Settings Record is embedded as JSON in each TIFF's `ImageDescription` tag instead of a
> `.json` file; `output` gains `inversion` and loses `share_copy`, `processing` loses
> `points`, `brightness`, `contrast` and `probably_blank`. Sections below that describe
> withdrawn items are kept for the record.

## Film Type (`FilmType`)

| Variant | Meaning |
|---------|---------|
| `BwNegative` | Silver-based black-and-white negative. The only type in this feature. |

A color negative type is added by a later feature. The parser recognises `color` only to reject
it (research P6).

## Film Stock (`FilmStock`)

| Value (`--film`) | Name shown | Type | Default |
|------------------|------------|------|---------|
| `bw` | Generic black-and-white negative | `BwNegative` | yes |
| `hp5` | Ilford HP5 Plus | `BwNegative` | |
| `fp4` | Ilford FP4 Plus | `BwNegative` | |

`color` → usage error "Color film is not supported yet." (exit 2). Anything else → clap's usage
error listing the valid values.

## Processing Profile (`Profile`)

A stock's defaults (spec FR-012). All three stocks start with these values (research P5, P8);
each stock has its own `match` arm so it can be tuned alone.

| Field | Start value | Meaning |
|-------|-------------|---------|
| `black_clip` | 0.001 | Fraction of picture-area pixels allowed darker than black |
| `white_clip` | 0.0005 | Fraction allowed lighter than white |
| `margin_mm` | 2.0 | Border ignored by the statistics on each side |
| `min_range` | 0.15 D | Below this density range the frame is "probably blank" |
| `nominal_range` | 1.2 D | Range used instead for a probably blank frame |
| `opaque_range` | 2.0 D | Pixels this much denser than black are dust, not silver: left out of the white point and the densest-silver measure |
| `brightness` | 0 | Default brightness |
| `contrast` | 0 | Default contrast |
| `dust` | `normal` | Default dust removal level |

## Processing Settings (`ProcessSettings`)

What the owner asked for, validated before the scanner opens (spec FR-020).

| Field | Source | Validation |
|-------|--------|------------|
| `stock` | `--film` | see Film Stock |
| `brightness` | `--brightness`, else profile | integer −50…+50 |
| `contrast` | `--contrast`, else profile | integer −50…+50 |
| `points` | `--black` + `--white`, else automatic | both or neither; each 0.00–4.00, kept to 3 decimals; black < white |
| `match_tones` | `roll --match-tones` | `roll` only; with explicit points it is implied and redundant |
| `dust` | `--dust`, else profile | `off`, `gentle`, `normal`, `strong` |

## Tone Points (`TonePoints`)

| Field | Notes |
|-------|-------|
| `black_density` | Density mapped to black, 3 decimals |
| `white_density` | Density mapped to white, 3 decimals |
| `source` | `auto` \| `explicit` \| `matched` (with `matched_from_frame`) \| `blank-default` |

State per roll run with `--match-tones`:

```text
Unlocked ──(frame not probably blank: its auto points)──▶ Locked(points, frame N)
Unlocked ──(probably blank frame)──▶ Unlocked   (that frame uses blank-default points)
Locked   ──(every later frame)──▶ Locked        (frame uses the locked points, source matched)
```

## Dust Level (`DustLevel`)

| Level | `margin` (D above the densest silver) | `max_area` | `max_span` | `dilate` |
|-------|--------------------------|------------|------------|----------|
| `off` | — | — | — | — |
| `gentle` | 0.50 | 0.05 mm² | 1.0 mm | 1 px |
| `normal` | 0.30 | 0.10 mm² | 2.0 mm | 2 px |
| `strong` | 0.15 | 0.25 mm² | 4.0 mm | 3 px |

mm values are converted to pixels with the frame's resolution, at least 1 px each (research P8).

## Gray Frame (`GrayFrame`)

The in-memory frame during processing (research P1).

| Field | Notes |
|-------|-------|
| `width`, `height` | From `FrameParams` |
| `dpi` | From width and scan area (002) |
| `pixels` | `Vec<u16>`, row-major, `width × height`; raw gray `(R+G+B)/3`, later the positive in place |
| `filled` | Samples received so far; the frame is complete only when `filled == width × height` |

Built from 8-bit (previews, scaled by 257) or 16-bit captures with 3 or 4 channels; partial
samples and pixels split across chunks carry over to the next chunk.

## Frame Result (`ProcessedFrame`)

| Field | Notes |
|-------|-------|
| `points` | `TonePoints` actually used |
| `probably_blank` | bool |
| `dust_repaired` | number of components repaired (0 when off) |
| `seconds` | processing time only |

## Saved Frame (`SavedFile`, changed)

Per frame, three files with one stem (research P10); stem rules from 002 D8, with `.jpg` added
to the free-name check.

| Field | Notes |
|-------|-------|
| `frame` | 1–999 (absent for previews) |
| `image` | master `<stem>.tif`: 16-bit gray (previews: 8-bit gray) |
| `share_copy` | `<stem>.jpg`, 8-bit gray JPEG (absent for previews) |
| `settings` | `<stem>.json` |
| `width`, `height` | pixels |
| `channels` | `"Gray"` |
| `bits` | 16 (previews 8) |
| `film` | stock value, e.g. `hp5` |
| `probably_blank` | bool |
| `dust_repaired` | count |
| `seconds` | capture + processing + writing |
| `film_moved` | `advanced` \| `none` (002) |

Publication order: `.json`, `.jpg`, `.tif`. `next_frame` still counts `.tif` files only, so a
frame counts as saved only when its master exists.

## Settings Record (sidecar `.json`, `record_version` 2)

002 D10's fields stay, at the top level, describing the **capture** (`color_mode` is now
`RGB` or `RGBI` of the capture, `width`/`height`/`resolution_dpi` of the frame). New fields:

```json
{
  "record_version": 2,
  "...002 capture fields...": "...",
  "output": {"kind": "positive-master", "channels": "Gray", "bits_per_sample": 16,
             "encoding": "display gray (sRGB-like)", "share_copy": "2026-11-09-003.jpg"},
  "processing": {
    "film_type": "bw-negative",
    "film_stock": "hp5",
    "profile": {"black_clip": 0.001, "white_clip": 0.0005, "margin_mm": 2.0,
                "min_range": 0.15, "nominal_range": 1.2, "opaque_range": 2.0},
    "points": {"black_density": 0.142, "white_density": 1.387, "source": "matched",
               "matched_from_frame": 1},
    "brightness": 0,
    "contrast": 10,
    "dust": {"level": "normal", "margin": 0.3, "max_area_px": 2010, "max_span_px": 283,
             "dilate_px": 2, "repaired": 17},
    "probably_blank": false,
    "seconds": 2.4
  }
}
```

Previews: `output.kind` = `preview`, `bits_per_sample` 8, no `share_copy`, `dust.level` `off`.
With the hidden `--keep-capture`, the record also names the kept raw file under
`processing.kept_capture`.

## Run Outcome (changed)

Outcomes and exit codes are 002's. Processing problems after a capture (an impossible frame
size, an encoder failure) are reported as `scan-failed` (exit 9) or `output-error` (exit 10)
naming the frame to rescan; no new outcome. The run report gains `film` and, for matched
rolls, the locked points used in the resume command.
