# primefilm

A command-line tool for the **Pacific Image PrimeFilm XA** film scanner on Linux, built on the
open SANE `pieusb` driver. It is also a Rust learning project: the code is commented as a
teaching text (see [the constitution](.specify/memory/constitution.md), Principle VI).

Current features:

- `primefilm detect` checks that the scanner is connected and usable.
- `primefilm capabilities` lists every setting the scanner exposes, starting with a summary of
  the ones that matter for film (resolutions, color modes, bit depths, scan area, infrared).
- `primefilm scan`, `roll`, `preview`, `advance` and `presets` scan film: one frame, a whole
  strip, a quick preview, or just move the film on (see [Scanning](#scanning)).
- Black-and-white negatives come out as positives with automatic black and white points,
  ready for finishing in post (see [Black-and-white processing](#black-and-white-processing)); `primefilm films`
  lists the film stocks.

Every frame is saved as 16-bit gray positive TIFFs, with every capture setting recorded inside
each file. The raw negative is not kept, and no existing file is ever replaced. Color film is
not supported yet.

## Prerequisites

- Rust stable via [rustup](https://rustup.rs) (`cargo --version` works)
- SANE runtime and headers: `sudo apt install libsane1 libsane-dev`
- The scanner on USB (it shows up as `05e3:0144` in `lsusb`), with VueScan closed

## Build

```bash
cargo build --release
```

The binary is `target/release/primefilm`. During development, `cargo run -q -- <args>` works too.

## Usage

```bash
primefilm detect
```

```text
Found 1 scanner:
  pieusb:libusb:001:008  PIE MF Scanner (film scanner)
```

```bash
primefilm capabilities
```

prints the scanner name, a `Summary` block, then every option grouped by section, with its
current value, allowed values and title. Inactive options are listed as `(inactive)`.

Options:

| Flag | Effect |
|------|--------|
| `--json` | Print one JSON document on stdout instead of text (schema in [output.schema.json](specs/001-scanner-detect/contracts/output.schema.json)) |
| `-v` | Show what the tool is doing on stderr |
| `-vv` | Also turn on the pieusb driver's own debug log |
| `--device <NAME>` | (`capabilities`) choose a scanner when more than one is connected |

```bash
primefilm --json capabilities | python3 -m json.tool
```

Failures print a message and a next step on stderr and exit with a distinct code (3 not found,
4 busy, 5 permission denied, 6 driver unavailable, 7 disconnected, 130 interrupted, 1 other
SANE error). The full table is in
[contracts/cli.md](specs/001-scanner-detect/contracts/cli.md#outcomes-and-exit-codes).

## Scanning

Insert a film strip and let the scanner pull it in (wait a few seconds before scanning). Eject
it with the scanner's own button; the tool never ejects.

| Command | What it does | Film |
|---------|--------------|------|
| `primefilm preview` | Quick 300 dpi look at the frame in the scanner (about 12 s), saved to `previews/` | stays put |
| `primefilm scan` | Scan the frame in the scanner once | stays put |
| `primefilm roll` | Scan, advance, repeat until the end of the strip | moves one frame after each scan |
| `primefilm roll --preview` | The same at preview speed (300 dpi, about 12 s a frame), saved to `previews/<roll>/` | moves one frame after each scan |
| `primefilm advance` | Move the film on one frame, saving nothing (about 30 s) | moves one frame |
| `primefilm presets` | List the quality presets | – |
| `primefilm films` | List the film stocks | – |

```bash
primefilm roll --roll hp5-01 --film hp5
```

```text
Roll hp5-01 → /home/you/Pictures/primefilm/hp5-01 (preset standard, 3600 dpi, Ilford HP5 Plus, dust off)
Frame 001: scanning…
Frame 001: saved hp5-01-001.tif (5173 × 3443, gray 16-bit) in 92 s; advancing film
…
Frame 006: same as the previous frame; end of film.
Saved 5 frames (001–005) to /home/you/Pictures/primefilm/hp5-01 in 9 min 10 s.
Reached the end of the film after frame 005.
```

The PrimeFilm XA does not report the end of the strip; it just stays on the last frame. So the
tool scans until a frame matches the one before it (or shows no film), and discards that
repeat. Use `--frames N` to stop after N frames.

`primefilm roll --preview --roll hp5-01` runs through the whole strip at preview speed, to see
what is on it before committing to full scans. The 8-bit previews go to
`previews/hp5-01/hp5-01-001.tif`, …, never into the roll's archive folder.
It takes no `--preset`, `--resolution` or `--dust`. Reload the strip afterwards to scan it for
real.

**Presets** change only the resolution (`--preset web|standard|high`, or
`--resolution DPI` for anything from 25 to 5000):

| Preset | dpi | Pixels | Each positive | Time per frame |
|--------|-----|--------|----------|----------------|
| `web` | 1800 | 2586 × 1721 | 9 MB | 49 s |
| `standard` (default) | 3600 | 5173 × 3443 | 36 MB | 91 s |
| `high` | 5000 | 7185 × 4783 | 69 MB | 5 min 14 s |

There is no higher setting: the scanner advertises 10000 dpi, but its sensor resolves at most
5000 dpi across the frame, and above that the images come out stretched.

A 36-frame roll at `standard` is about 1.3 GB. The scanner driver also holds each raw frame in `/tmp` while scanning (275 MB at
`high`), so keep some space free there.

**Files** go to `$HOME/Pictures/primefilm/` (change with `--output DIR`):

```text
<roll>/<roll>-001.tif                  the positive: 16-bit gray, uncompressed, full resolution
previews/preview-20261109-120248.tif   8-bit gray positive, not archival
```

Each TIFF carries every setting used (preset, resolution, exposure, gain, scanner, time, film,
dust removal) as JSON in its `ImageDescription` tag, e.g. `tiffinfo <file>`; there are no
sidecar files.

`--roll` defaults to today's date. Frame numbers continue from the highest one already in the
roll's folder, so a second `scan` or a resumed `roll` just keeps counting; `--frame N` /
`--start-frame N` choose the number. If a name is taken (rescanning a frame), `-2`, `-3`, … is
added. Files are written under a hidden `.partial` name and only appear when complete.

**Stopping and resuming**: Ctrl-C during a roll finishes and saves the frame in progress (the
scanner has already moved the film on by then), then stops; press it again to abort at once.
After Ctrl-C, a disconnect or a scanner error, the tool prints the command that continues the
roll, e.g. `primefilm roll --roll hp5-01 --start-frame 4 --film hp5`, repeating any processing
options you gave.

New exit codes: 8 no film, 9 the scanner reported an error during a scan, 10 a file could not
be written. Every command takes `--json`; the format is in
[scan-output.schema.json](specs/003-bw-processing/contracts/scan-output.schema.json)
(version 2), and the command reference in
[002 contracts/cli.md](specs/002-film-scan/contracts/cli.md) and
[003 contracts/cli.md](specs/003-bw-processing/contracts/cli.md).

## Black-and-white processing

Each frame is turned into a positive as soon as it is scanned:

1. The red, green and blue channels are averaged into one gray image. Silver is neutral, so
   all three hold the same picture, and any color cast disappears.
2. Black and white points are set from the picture (ignoring a 2 mm border): the clear film
   base becomes black and the densest highlights white, with density mapped evenly in between.
   A frame with almost no tonal range is reported as "probably blank" instead of being
   stretched into grain. There are no brightness or contrast settings; finish in post.
3. Dust removal, only if asked for (`--dust`, off by default). Dust is opaque, so it should be
   denser than any silver on the frame; small specks and hairs that dense are filled in from
   their surroundings. It is off by default until it has been tested and tuned on real HP5 and
   FP4 captures.

| Option | Commands | What it does |
|--------|----------|--------------|
| `--film bw\|hp5\|fp4` | scan, roll, preview | The film stock (default `bw`, any B&W negative); recorded with each frame |
| `--dust off\|gentle\|normal\|strong` | scan, roll | Dust removal strength (default `off`) |

HP5 and FP4 currently share one processing profile; each has its own entry in
[src/film.rs](src/film.rs) so it can be tuned separately.

## Permissions

The tool runs as a normal user. If it reports "No permission to access /dev/bus/usb/…", add a
udev rule so your user can open the scanner:

```bash
echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="05e3", ATTR{idProduct}=="0144", MODE="0666"' | sudo tee /etc/udev/rules.d/60-primefilm.rules
```

```bash
sudo udevadm control --reload-rules
```

Then unplug and replug the scanner.

## Troubleshooting

### "The scanner is in use by another program" right after connecting it

On Ubuntu, the color-management service `colord` starts a helper, `colord-sane`, whenever a
scanner connects. The helper loads every SANE driver and claims the scanner, so `primefilm`
reports it as busy (exit 4). Check with:

```bash
pgrep -a colord-sane
```

The fix gives colord an empty SANE configuration, so it loads no scanner drivers. colord keeps
managing display and printer color profiles.

```bash
sudo mkdir -p /etc/colord-no-sane /etc/systemd/system/colord.service.d
```

```bash
echo '# Empty on purpose: colord loads no scanner drivers.' | sudo tee /etc/colord-no-sane/dll.conf
```

```bash
printf '[Service]\nEnvironment=SANE_CONFIG_DIR=/etc/colord-no-sane\n' | sudo tee /etc/systemd/system/colord.service.d/no-scanners.conf
```

```bash
sudo systemctl daemon-reload && sudo systemctl restart colord
```

Confirm it took effect (it should print `Environment=SANE_CONFIG_DIR=/etc/colord-no-sane`),
then switch the scanner off and on:

```bash
systemctl show colord -p Environment
```

To undo it:

```bash
sudo rm -r /etc/systemd/system/colord.service.d/no-scanners.conf /etc/colord-no-sane && sudo systemctl daemon-reload && sudo systemctl restart colord
```

Details are in [research.md](specs/001-scanner-detect/research.md), findings of 2026-11-09.

### The scanner drops off USB about 90 seconds after connecting

This has been seen repeatedly and the cause is not known yet. Switch the scanner off and on
and run the command again. See [research.md](specs/001-scanner-detect/research.md), risk R3.
If it happens during a roll, the frames already saved are kept and the tool prints the command
that resumes the roll.

### "The scanner is not ready" right after inserting film

The scanner is still pulling the strip in. Wait a few seconds and run the command again.

## Why only pieusb is loaded

With every SANE driver loaded, another driver can grab the scanner before pieusb does
("Device busy"), and webcams appear as scanners. On each run `primefilm` writes a private SANE
config to `$XDG_RUNTIME_DIR/primefilm/sane.d/` (from this repo's [sane.d/](sane.d/)) that loads
only pieusb. [scripts/probe.sh](scripts/probe.sh) does the same for `scanimage`, as an
independent cross-check.

## Tests

```bash
cargo test
```

runs everything that needs no scanner: unit tests, the CLI against a simulated scanner, and the
real SANE code against SANE's built-in `test` driver. With the scanner on:

```bash
cargo test -- --ignored
```

Before committing, all three must pass:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## How the code is organized

Read the modules in this order; each Rust concept is explained in a comment where it first
appears.

1. [src/lib.rs](src/lib.rs): the module tree, and `run`, which picks a backend and runs the
   command. Modules, `pub`, ownership and borrowing, generics.
2. [src/error.rs](src/error.rs): `ScanError`, every way a run can fail, with its message, next
   step and exit code. Enums, `match`, `Result` and `?`, unit tests.
3. [src/cli.rs](src/cli.rs): the command-line arguments, defined with clap. `#[derive]`.
4. [src/backend/mod.rs](src/backend/mod.rs): the `ScannerBackend` and `ScanSession` traits,
   the one interface all scanner access goes through. Structs, traits, associated types.
5. [src/capability.rs](src/capability.rs): scanner options and the film summary. Iterators and
   closures, serde attributes.
6. [src/backend/fake.rs](src/backend/fake.rs): the simulated scanner used by tests, with
   [fixtures/primefilm-xa.json](src/backend/fixtures/primefilm-xa.json).
7. [src/report.rs](src/report.rs): text and JSON output. Serde.
8. [src/preset.rs](src/preset.rs): the quality presets. Associated constants, `Display`.
9. [src/tiff.rs](src/tiff.rs): the streaming TIFF writer. `Write` and `Seek`, byte order.
10. [src/frame_stats.rs](src/frame_stats.rs): the "no film" and "same frame" checks.
11. [src/output.rs](src/output.rs): folders, numbering, `.partial` files published with a hard
    link so nothing is ever overwritten. `std::fs`, `Drop`.
12. [src/film.rs](src/film.rs): film types, stocks, profiles and dust levels. Enums as the
    place new films plug in, `impl Default`-style constant profiles.
13. [src/gray.rs](src/gray.rs): the in-memory gray frame. A flat `Vec<u16>` as a 2-D image,
    `chunks_exact`, checked conversions.
14. [src/tone.rs](src/tone.rs): density, the histogram, black and white points, and the tone
    lookup table. Floating-point maths, running totals, tables built from
    iterators.
15. [src/dust.rs](src/dust.rs): dust removal. A flood fill with an explicit stack,
    `saturating_sub` at the edges.
16. [src/process.rs](src/process.rs): the per-frame processing.
17. [src/scan.rs](src/scan.rs): the `scan`, `roll`, `preview` and `advance` sequences, written
    once against the backend traits. Reused buffers, `Option` defaults.
18. [src/main.rs](src/main.rs): the binary: SANE config, Ctrl-C handling, exit codes. `Arc` and
   atomics.
19. [src/usb.rs](src/usb.rs): reads `/sys` to tell "off" from "busy" from "no permission".
20. [src/backend/sane/ffi.rs](src/backend/sane/ffi.rs): raw C declarations from `sane.h`. FFI,
    `#[repr(C)]`, raw pointers.
21. [src/backend/sane/config.rs](src/backend/sane/config.rs): writes the pieusb-only SANE config.
22. [src/backend/sane/mod.rs](src/backend/sane/mod.rs): the safe wrapper (`Sane`, `Device`),
    `SaneBackend` and `SaneSession`. `unsafe` and `// SAFETY:`, `Drop` and drop order, moving
    ownership.
23. [examples/process_capture.rs](examples/process_capture.rs): replays a raw capture kept with
    the hidden `--keep-capture DIR` flag through the processing, for tuning. Examples.
24. [tests/](tests/): integration tests that run the real binary.

Design notes, decisions and hardware findings are in
[specs/001-scanner-detect/](specs/001-scanner-detect/),
[specs/002-film-scan/](specs/002-film-scan/) and
[specs/003-bw-processing/](specs/003-bw-processing/).
