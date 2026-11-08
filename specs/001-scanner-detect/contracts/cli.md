# CLI Contract: `primefilm`

**Feature**: [../spec.md](../spec.md) | **Data model**: [../data-model.md](../data-model.md) |
**JSON schema**: [output.schema.json](output.schema.json)

## Synopsis

```text
primefilm [OPTIONS] <COMMAND>

Commands:
  detect          Check whether the PrimeFilm XA is connected and usable
  capabilities    List every setting the scanner exposes
  help            Print help

Options:
  -v, --verbose   Show detection steps on stderr; repeat (-vv) to add SANE driver debug output
      --json      Print machine-readable JSON on stdout
  -h, --help      Print help
  -V, --version   Print version

capabilities options:
      --device <NAME>   SANE device name to query (default: the first scanner found)
```

Global options may appear before or after the subcommand. The tool never reads stdin and never
prompts (FR-009).

### Hidden flags (for tests; not shown in `--help`)

| Flag | Effect |
|------|--------|
| `--simulate <SCENARIO>` | Use the fake backend instead of SANE. Scenarios: `ready`, `no-scanner`, `busy`, `permission-denied`, `driver-missing`, `disconnect`, `interrupt`. |
| `--sane-backend <NAME>` | Load this SANE backend instead of `pieusb` (tests use `test`). Skips the `/sys` checks, which only apply to the PrimeFilm XA. |

## Behavior

### `detect`

Lists matching scanners without opening them. Success when at least one is found.

Human output (stdout):

```text
Found 1 scanner:
  pieusb:libusb:001:004  Reflecta RPS 10M (film scanner)
```

### `capabilities`

Opens one scanner, reads every option's descriptor and current value (read-only), closes it,
then prints a summary and the full list. If several scanners are found and `--device` is not
given, it uses the first and says so on stderr.

Human output (stdout), shape only:

```text
Reflecta RPS 10M  (pieusb:libusb:001:004)

Summary
  Resolutions   7200, 3600, 1800 dpi
  Color modes   Color, Gray, RGBI
  Bit depths    8, 16
  Scan area     36.0 × 24.0 mm
  Infrared      available

Scan Mode
  mode         Color          [Color, Gray, RGBI]          Scan mode
  resolution   3600 dpi       [7200, 3600, 1800]           Sets the resolution
  ...
Advanced
  gamma-table  (inactive)     ...
```

Values shown above are illustrative only; real ones come from the scanner.

## Outcomes and exit codes

Checked in this order. With `--json`, failures still print one JSON document on stdout; the
human message always goes to stderr.

| Exit | `outcome` | When | Message (stderr) | Next step |
|------|-----------|------|------------------|-----------|
| 0 | `found` | Scanner(s) listed, and for `capabilities` opened and read | (none) | |
| 1 | `error` | Any other SANE failure | `SANE error: <status text>` | Re-run with `-vv` and check the output |
| 2 | (clap) | Invalid arguments | clap's usage message | See `primefilm --help` |
| 3 | `not-found` | `05e3:0144` not on USB | No PrimeFilm XA found on USB. | Check the scanner is powered on and the cable is connected; try a port directly on the computer. |
| 4 | `busy` | Interface already claimed (`usbfs`) or `SANE_STATUS_DEVICE_BUSY` | The scanner is in use by another program. | Close VueScan or other scanning programs and try again. |
| 5 | `permission-denied` | Device node not read-writable, or `SANE_STATUS_ACCESS_DENIED` | No permission to access /dev/bus/usb/BBB/DDD. | Add a udev rule for 05e3:0144 (see project docs) and replug the scanner. |
| 6 | `driver-unavailable` | `libsane-pieusb.so.1` missing, or scanner on USB but pieusb did not recognize it | The SANE pieusb driver is missing / did not recognize the scanner. | Install `libsane1`; or run `primefilm -vv detect` and check the model number in `sane.d/pieusb.conf`. |
| 7 | `disconnected` | Failure while reading and the scanner is gone from USB | The scanner disconnected while <reading options>. | Check power and cable, then run the command again. |
| 130 | `interrupted` | Ctrl-C | Interrupted; scanner released. | |

Exit 2 is reserved for clap and never produces JSON.

## JSON output

One document per run on stdout, valid against [output.schema.json](output.schema.json).

`detect` success:

```json
{
  "schema_version": 1,
  "outcome": "found",
  "devices": [
    {"name": "pieusb:libusb:001:004", "vendor": "Reflecta", "model": "RPS 10M", "kind": "film scanner"}
  ]
}
```

`capabilities` success: `schema_version`, `outcome`, `device`, `summary`, `options`
(see [data-model.md](../data-model.md)).

Failure:

```json
{
  "schema_version": 1,
  "outcome": "busy",
  "message": "The scanner is in use by another program.",
  "next_step": "Close VueScan or other scanning programs and try again."
}
```

`schema_version` increases only for incompatible changes; new fields may be added without it.

## Verbose output (`-v`)

Steps prefixed with `primefilm:` on stderr, for example:

```text
primefilm: SANE config dir /run/user/1000/primefilm/sane.d (backend: pieusb)
primefilm: USB 05e3:0144 found at bus 001 device 004, interface not claimed
primefilm: sane_init ok (SANE 1.0.x)
primefilm: 1 device(s) listed
primefilm: opened pieusb:libusb:001:004; reading 42 options
```

`-vv` also sets `SANE_DEBUG_PIEUSB=5` before SANE starts, so the driver's own log appears.
