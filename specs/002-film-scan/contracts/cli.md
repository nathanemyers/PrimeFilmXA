# CLI Contract: scanning commands

**Feature**: [../spec.md](../spec.md) | **Data model**: [../data-model.md](../data-model.md) |
**JSON schema**: [scan-output.schema.json](scan-output.schema.json) | **Base contract**:
[feature 001 cli.md](../../001-scanner-detect/contracts/cli.md)

Adds five subcommands to `primefilm`. Global options (`-v`, `--json`, hidden `--simulate`,
`--sane-backend`) and the 001 commands are unchanged. Nothing prompts or reads stdin.

## Synopsis

```text
primefilm scan     [--roll NAME] [--frame N] [--preset P | --resolution DPI] [SCAN OPTIONS]
primefilm roll     [--roll NAME] [--start-frame N] [--frames N] [--preset P | --resolution DPI] [SCAN OPTIONS]
primefilm preview  [SCAN OPTIONS]
primefilm advance  [--device NAME]
primefilm presets

SCAN OPTIONS:
  --output DIR     Output root (default: $HOME/Pictures/primefilm)
  --device NAME    SANE device name (default: the first scanner found)
```

| Command | Does | Film movement |
|---------|------|---------------|
| `scan` | Scan the frame in the scanner once, save it | none |
| `roll` | Scan, advance, repeat until the end of the film (or `--frames N` frames) | one frame after each scan |
| `preview` | Quick low-resolution scan of the frame in the scanner, save to `previews/` | none |
| `advance` | Move the film forward one frame; no file written | one frame |
| `presets` | List presets; does not touch the scanner | none |

Defaults: `--roll` = today's local date (`YYYY-MM-DD`); `--frame` / `--start-frame` = one past
the highest frame number already in the roll folder; `--preset standard`. `--preset` and
`--resolution` are mutually exclusive (clap usage error, exit 2). An unknown preset, a
resolution outside 25–5000, an invalid roll name, or `--frames 0` is a usage error (exit 2)
raised before the scanner is touched.

## Hidden flags (tests only)

| Flag | Effect |
|------|--------|
| `--simulate <SCENARIO>` | 001 scenarios plus `no-film`, `jam`, `empty-gate`; `disconnect` and `interrupt` now also fire mid-roll (frame 3 and frame 2). |
| `--simulate-frames N` | Number of frames on the simulated film (default 6). |

## Output

### Progress (stderr)

```text
Roll 2026-11-09 → /home/nathan/Pictures/primefilm/2026-11-09/ (preset standard, 3600 dpi)
Frame 001: scanning…
Frame 001: saved 2026-11-09-001.tif (5173 × 3443, RGBI 16-bit) in 74 s; advancing film
Frame 002: scanning…
^C
Stopping after frame 002 is saved (press Ctrl-C again to abort now).
Frame 002: saved 2026-11-09-002.tif (5173 × 3443, RGBI 16-bit) in 73 s; advancing film
```

Every film movement is logged (FR-013): roll progress lines end `advancing film`, `-v` adds
`film advanced after frame NNN`, and `advance` reports whether the film moved. With `-v`, the option
values sent to the scanner and the SANE calls are logged as in feature 001.

### Summary (stdout)

```text
Saved 2 frames (001–002) to /home/nathan/Pictures/primefilm/2026-11-09/ in 2 min 27 s.
Interrupted. To continue: primefilm roll --roll 2026-11-09 --start-frame 3
```

`scan` prints the one path saved; `preview` prints the preview path; `advance` prints
`Film advanced one frame.`; `presets` prints the preset table from the data model.

### JSON (`--json`, stdout)

One document per run, valid against [scan-output.schema.json](scan-output.schema.json):

```json
{
  "schema_version": 1,
  "outcome": "interrupted",
  "message": "Interrupted after frame 002; scanner released.",
  "next_step": "primefilm roll --roll 2026-11-09 --start-frame 3",
  "roll": "2026-11-09",
  "folder": "/home/nathan/Pictures/primefilm/2026-11-09",
  "preset": "standard",
  "resolution_dpi": 3600,
  "files": [
    {"frame": 1, "image": ".../2026-11-09-001.tif", "settings": ".../2026-11-09-001.json",
     "width": 5173, "height": 3443, "channels": "RGBI", "bits": 16, "seconds": 74.2,
     "film_moved": "advanced"}
  ]
}
```

On success `next_step` is omitted. For failures before any scan starts (busy, not found, …),
the document has only `schema_version`, `outcome`, `message` and `next_step`, like feature
001's failure document. Progress lines never go to stdout.

`film_moved` is `advanced` for roll frames after which the next frame was different, and `none`
for the last frame of a roll (the film could not move further), single scans and previews.

## Outcomes and exit codes

Feature 001's table applies (1, 3–7, 130). New and changed rows:

| Exit | `outcome` | When | Message (stderr) | Next step |
|------|-----------|------|------------------|-----------|
| 0 | `completed` | `scan`, `preview`, `advance` finished; `presets` | | |
| 0 | `end-of-film` | `advance` found the film already on its last frame | Already at the last frame; the film did not move. | |
| 0 | `end-of-film` | `roll` reached the end of the film or the `--frames` limit | Reached the end of the film after frame NNN. | |
| 7 | `disconnected` | Scanner left USB during a scan | The scanner disconnected while scanning frame NNN. | Check power and cable; check where the film is, then `primefilm roll --roll R --start-frame N`. |
| 8 | `no-film` | First frame finds no film | No film found in the scanner. | Insert a film strip and wait for the scanner to load it. |
| 9 | `scan-failed` | Scanner error during a frame while still on USB (jam, rejected settings) | The scanner reported an error while scanning frame NNN: <status>. | Check the film for a jam (use the eject button if needed), then resume as shown. |
| 10 | `output-error` | A folder or file cannot be created or written | Could not write <path>: <reason>. | Check free space and permissions of the output folder. |
| 130 | `interrupted` | Ctrl-C; the frame in progress was saved first (second Ctrl-C: aborted, frame lost) | Interrupted after frame NNN; scanner released. | the resume command |

`roll` reports `end-of-film` with exit 0 when a frame matches the previous one (the XA stays on
its last frame instead of reporting the end, research D4) or shows an empty gate; that frame is
not saved. `advance` reports `end-of-film` with exit 0 and "Already at the last frame; the film
did not move." when the frame in the gate did not change.

## Guarantees

- No existing file is ever replaced; a taken name gets `-2`, `-3`, … (research D7, D8).
- A `.tif` exists only if it is complete and its `.json` is beside it.
- `scan` and `preview` set `advance` off; only `roll` and `advance` move film.
- The scanner is released on every exit path (feature 001 FR-007).
