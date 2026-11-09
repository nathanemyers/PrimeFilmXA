# CLI Contract: B&W processing options

**Feature**: [../spec.md](../spec.md) | **Data model**: [../data-model.md](../data-model.md) |
**JSON schema**: [scan-output.schema.json](scan-output.schema.json) (version 2) | **Base
contract**: [feature 002 cli.md](../../002-film-scan/contracts/cli.md)

Feature 002's commands keep their names, flags, defaults, film movement, outcomes and exit
codes. This feature changes what `scan`, `roll` and `preview` save, adds processing options to
them, and adds a `films` command. Nothing prompts or reads stdin.

## Synopsis

```text
primefilm scan     [--roll NAME] [--frame N] [--preset P | --resolution DPI] [PROCESS OPTIONS] [SCAN OPTIONS]
primefilm roll     [--roll NAME] [--start-frame N] [--frames N] [--preset P | --resolution DPI]
                   [--match-tones] [PROCESS OPTIONS] [SCAN OPTIONS]
primefilm preview  [--film STOCK] [--brightness N] [--contrast N] [--black D --white D] [SCAN OPTIONS]
primefilm advance  [--device NAME]
primefilm presets
primefilm films

PROCESS OPTIONS:
  --film STOCK       bw (default), hp5, fp4
  --brightness N     -50..50, default 0: lighter (+) or darker (-) midtones
  --contrast N       -50..50, default 0: more (+) or less (-) midtone contrast
  --black D          Density shown as black, 0.00..4.00 (with --white; default: automatic)
  --white D          Density shown as white, 0.00..4.00 (with --black; must be above --black)
  --dust LEVEL       off, gentle, normal (default), strong
```

| Option | Applies to | Notes |
|--------|------------|-------|
| `--film` | scan, roll, preview | `color` → "Color film is not supported yet." exit 2 |
| `--brightness`, `--contrast` | scan, roll, preview | out of range → exit 2 |
| `--black`, `--white` | scan, roll, preview | only one given, out of range, or black ≥ white → exit 2 |
| `--dust` | scan, roll | previews never remove dust |
| `--match-tones` | roll | on `scan` → clap usage error (exit 2) |

All checks happen before the scanner is opened or the film moves.

## Hidden flags (development and tests only)

002's `--simulate` and `--simulate-frames` are unchanged; the simulated frames now look like
negatives with a clear border and a few dust specks.

| Flag | Effect |
|------|--------|
| `--keep-capture DIR` | Also save each raw capture as an RGB(I) TIFF in `DIR` for tuning and fixtures (research P12). Never used in normal scanning. |

## Files written

Per frame in `<output>/<roll>/` (stem rules from 002, `-2`, `-3`, … if any of the three names
is taken):

| File | Contents |
|------|----------|
| `<stem>.tif` | Positive master: 16-bit gray, uncompressed, full resolution |
| `<stem>.jpg` | Share copy: 8-bit gray JPEG, full resolution |
| `<stem>.json` | Settings record version 2 (capture + processing) |

Previews: `<output>/previews/preview-<YYYYMMDD-HHMMSS>.tif` (8-bit gray positive) and `.json`.

No raw capture and no `.partial` file is left after any run, including failed and interrupted
ones (spec SC-007).

## Output

### Progress (stderr)

```text
Roll 2026-11-09 → /home/nathan/Pictures/primefilm/2026-11-09/ (preset standard, 3600 dpi, HP5, dust normal, matching tones)
Frame 001: scanning…
Frame 001: probably blank; saved 2026-11-09-001.tif + .jpg (5173 × 3443, gray 16-bit) in 94 s; advancing film
Frame 002: scanning…
Frame 002: tones locked (black 0.142 D, white 1.387 D); 17 dust specks removed; saved 2026-11-09-002.tif + .jpg in 93 s; advancing film
```

### Summary (stdout)

```text
Saved 2 frames (001–002) to /home/nathan/Pictures/primefilm/2026-11-09/ in 3 min 07 s.
Probably blank: 001.
Interrupted. To continue: primefilm roll --roll 2026-11-09 --start-frame 3 --film hp5 --black 0.142 --white 1.387
```

The resume command repeats every processing option that was given, plus the locked points of
a matched roll in place of `--match-tones`.

`films` prints:

```text
Film   Name                              Type
bw     Generic black-and-white negative  B&W negative  (default)
hp5    Ilford HP5 Plus                   B&W negative
fp4    Ilford FP4 Plus                   B&W negative
Color film is not supported yet.
```

### JSON (`--json`, stdout)

`schema_version` 2, valid against [scan-output.schema.json](scan-output.schema.json):

```json
{
  "schema_version": 2,
  "outcome": "end-of-film",
  "message": "Reached the end of the film after frame 002.",
  "roll": "2026-11-09",
  "folder": "/home/nathan/Pictures/primefilm/2026-11-09",
  "preset": "standard",
  "resolution_dpi": 3600,
  "film": "hp5",
  "tone_points": {"black_density": 0.142, "white_density": 1.387, "matched_from_frame": 2},
  "files": [
    {"frame": 2, "image": ".../2026-11-09-002.tif", "share_copy": ".../2026-11-09-002.jpg",
     "settings": ".../2026-11-09-002.json", "width": 5173, "height": 3443,
     "channels": "Gray", "bits": 16, "film": "hp5", "probably_blank": false,
     "dust_repaired": 17, "seconds": 93.4, "film_moved": "none"}
  ]
}
```

`films --json` prints `{"schema_version": 2, "outcome": "completed", "films": [{"value": "bw",
"name": "Generic black-and-white negative", "film_type": "bw-negative", "default": true}, …]}`.

## Outcomes and exit codes

Unchanged from 002. Additions:

| Exit | `outcome` | When |
|------|-----------|------|
| 2 | (clap usage error) | `--film color`, unknown stock, values out of range, `--black` without `--white`, black ≥ white, `--match-tones` on `scan` |
| 9 | `scan-failed` | Processing found the captured frame unusable (for example zero size); message names the frame to rescan |
| 10 | `output-error` | The master, share copy or record could not be written |

A frame is never reported as saved unless all three of its files were published.
