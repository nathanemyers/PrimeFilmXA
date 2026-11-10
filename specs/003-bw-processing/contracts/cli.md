# CLI Contract: B&W processing options

**Feature**: [../spec.md](../spec.md) | **Data model**: [../data-model.md](../data-model.md) |
**JSON schema**: [scan-output.schema.json](scan-output.schema.json) (version 2) | **Base
contract**: [feature 002 cli.md](../../002-film-scan/contracts/cli.md)

*Revised 2026-11-09 (research P15, P16): tone options (`--brightness`, `--contrast`,
`--black`, `--white`, `--match-tones`), the JPEG share copy and the JSON sidecar are removed;
dust removal is off by default; each frame is one automatically toned TIFF.*

Feature 002's commands keep their names, flags, defaults, film movement, outcomes and exit
codes. This feature changes what `scan`, `roll` and `preview` save, adds two options, and adds
a `films` command. Nothing prompts or reads stdin.

## Synopsis

```text
primefilm scan     [--roll NAME] [--frame N] [--preset P | --resolution DPI] [--film STOCK] [--dust LEVEL] [SCAN OPTIONS]
primefilm roll     [--roll NAME] [--start-frame N] [--frames N] [--preset P | --resolution DPI]
                   [--film STOCK] [--dust LEVEL] [SCAN OPTIONS]
primefilm roll     --preview [--roll NAME] [--start-frame N] [--frames N] [--film STOCK] [SCAN OPTIONS]
primefilm preview  [--film STOCK] [SCAN OPTIONS]
primefilm advance  [--device NAME]
primefilm presets
primefilm films
```

| Option | Applies to | Notes |
|--------|------------|-------|
| `--film bw\|hp5\|fp4` | scan, roll, preview | Default `bw`. `color` → "Color film is not supported yet." exit 2 |
| `--dust off\|gentle\|normal\|strong` | scan, roll | Default `off`. Previews never remove dust |
| `--preview` | roll | Roll preview: each frame an ordinary 300 dpi 8-bit scan that advances the film, saved as previews in `<output>/previews/<roll>/<roll>-NNN.tif`. Conflicts with `--preset`, `--resolution`, `--dust` (exit 2). JSON omits `preset`/`resolution_dpi`; the resume command adds `--preview`. |

All checks happen before the scanner is opened or the film moves. The removed tone options
are now unknown arguments (clap usage error, exit 2).

## Hidden flags (development and tests only)

002's `--simulate` and `--simulate-frames` are unchanged; the simulated frames look like
negatives with a clear border and three opaque one-pixel dust specks.

| Flag | Effect |
|------|--------|
| `--keep-capture DIR` | Also save each raw capture as an RGB(I) TIFF in `DIR` for tuning and fixtures (research P12). Never used in normal scanning. |

## Files written

Per frame in `<output>/<roll>/` (stem rules from 002; `-2`, `-3`, … if any of the frame's names
is taken):

| File | Contents |
|------|----------|
| `<stem>.tif` | Positive with automatic black and white points, 16-bit gray, uncompressed, full resolution |

Each TIFF's `ImageDescription` tag holds the settings record (version 2) as compact JSON. No
`.json` or `.jpg` files are written.

Previews: `<output>/previews/preview-<YYYYMMDD-HHMMSS>.tif`, 8-bit gray.

No raw capture and no `.partial` file is left after any run, including failed and interrupted
ones (spec SC-007).

## Output

### Progress (stderr)

```text
Roll 2026-11-09 → /home/nathan/Pictures/primefilm/2026-11-09/ (preset standard, 3600 dpi, Ilford HP5 Plus, dust off)
Frame 001: scanning…
Frame 001: saved 2026-11-09-001.tif (5173 × 3443, gray 16-bit) in 93 s; advancing film
```

A probably blank frame adds `probably blank; ` before `saved`; with `--dust`, a frame where
specks were repaired adds `N dust specks removed; `.

### Summary (stdout)

As in 002. The resume command repeats `--film` and `--dust` if they were given, e.g.
`primefilm roll --roll 2026-11-09 --start-frame 3 --film hp5`.

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
  "files": [
    {"frame": 2, "image": ".../2026-11-09-002.tif", "width": 5173, "height": 3443,
     "channels": "Gray", "bits": 16, "film": "hp5", "probably_blank": false,
     "dust_repaired": 0, "seconds": 93.4, "film_moved": "none"}
  ]
}
```

`films --json` prints `{"schema_version": 2, "outcome": "completed", "films": [{"value": "bw",
"name": "Generic black-and-white negative", "film_type": "bw-negative", "default": true}, …]}`.

## Outcomes and exit codes

Unchanged from 002. Additions:

| Exit | `outcome` | When |
|------|-----------|------|
| 2 | (clap usage error) | `--film color`, unknown stock, unknown dust level, `--dust` on `preview`, any removed tone option |
| 9 | `scan-failed` | Processing found the captured frame unusable (for example zero size); message names the frame to rescan |
| 10 | `output-error` | A positive could not be written |

A frame is never reported as saved unless its positive was published.
