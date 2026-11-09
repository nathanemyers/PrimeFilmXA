# Research: Film Scanning (Single Frame, Roll, Preview)

**Feature**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md) | **Date**: 2026-11-09

Feature 001's findings ([../001-scanner-detect/research.md](../001-scanner-detect/research.md))
still apply: pieusb drives the XA, colord must be kept off the scanner, and the scanner tends
to drop off USB about 90 s after it enumerates (R3 there).

## Findings from the pieusb source (SANE `backend/pieusb*.c`, master, read 2026-11-09)

These come from reading the driver, not from the hardware. Each one marked **(H)** is verified
on the scanner before the code that depends on it is marked done (see Hardware experiments).

- **The whole frame is captured inside `sane_start`.** pieusb runs calibration, the scan and
  all USB data transfer in `sane_start`, into a memory-mapped temp file it creates with
  `mkstemp("/tmp/sane.XXXXXX")`. `sane_read` only copies bytes out of that file. Therefore:
  - progress inside a frame cannot be measured; the tool reports per frame (start, saved,
    seconds taken);
  - `sane_cancel` is only honored between scan phases, so Ctrl-C cannot stop a frame quickly;
  - pieusb's temp file is the full uncompressed frame (275 MB at 5000 dpi RGBI 16-bit). If
    `/tmp` is a tmpfs, that is RAM. Our own code still streams (constitution: bounded memory).
- **Film advance is part of `sane_start`, and only for "slide transport" devices.** After the
  data is read, pieusb sends `SLIDE NEXT` if the `advance` option is true and `preview` is
  false, and only if the device entry has `FLAG_SLIDE_TRANSPORT` (0x01). Before every scan it
  sends `SLIDE INIT` for such devices. The flag comes from the last field of the device line in
  `pieusb.conf` (`usb <vendor> <product> <model> <flags>`). **Our shipped line, copied from
  upstream, is `usb 0x05e3 0x0144 0x0048 0x00`: no slide transport.** With it, `advance` does
  nothing and the film never moves. Upstream also defines 0x02 "no CMD_17" for transport
  devices that reject command 0x17 (DigitDia 4000 uses `0x03`). **(H)**: which flags the XA
  needs, and what `SLIDE INIT` does to a loaded strip.
- **A failed `SLIDE NEXT` is only logged**, not returned: `sane_start` still returns GOOD.
  pieusb has no "no film" or "end of film" status; scanner sense codes it does not know become
  `SANE_STATUS_INVAL` or `SANE_STATUS_IO_ERROR`. **(H)**: what the XA reports when it scans
  past the last frame or with no film.
- **Preview** (`preview` = true) scans at the device's `fast_preview_resolution` (from its
  inquiry data; value unknown **(H)**), ignores `resolution`, never advances, and skips
  infrared post-processing. Its histogram is stored for the `calibration` mode
  "from preview"; with the default calibration mode ("from internal test") a preview does not
  change later scans.
- **RGBI data**: `sane_get_parameters` reports `format = RGB` for RGBI (the SANE standard has
  no RGBI frame), with `bytes_per_line = 2 × 4 × pixels_per_line` at 16 bits. Samples come
  pixel-interleaved (R, G, B, I for each pixel), 16-bit values in host byte order (little-endian
  on x86-64). The channel count must therefore be derived as
  `bytes_per_line / (pixels_per_line × bytes_per_sample)`, not from `format`.
- **Post-processing options are software, inside pieusb**: `clean-image` (dust removal),
  `correct-infrared` (remove red crosstalk from IR), and `smooth`/`crop`, which only take effect
  together with those two. Setting `clean-image` and `correct-infrared` to false gives raw
  data. `correct-shading` (flat-field correction from the scanner's own calibration strip) is
  part of capture and stays on, as in every scan VueScan and pieusb make.
- **Option changes are per session.** pieusb keeps option values in the process and sends them
  to the scanner as the scan mode for each `sane_start`; nothing is stored on the scanner, so
  setting options does not conflict with constitution Principle II.

## Hardware experiments (run before the code that depends on them)

Run with the repo's SANE config (`SANE_CONFIG_DIR` pointing at a copy of `sane.d/` with the
flags under test) and `scanimage`, a strip of 4–6 frames loaded, watching the scanner. Results
go in [hardware-validation.md](hardware-validation.md) and back into this file.

| # | Question | Experiment | Decides |
|---|----------|-----------|---------|
| E1 | Does the XA accept slide-transport commands? | Flags `0x01`: `scanimage --mode Color --depth 8 --resolution 300 --advance=no -o f1.tiff`. If `sane_start` fails on command 0x17 (`-vv` log), retry with `0x03`. | The flags value shipped in `sane.d/pieusb.conf` (D2). |
| E2 | Does `SLIDE INIT` move a loaded strip? | Same command twice with `--advance=no`; compare images and watch the film. | Whether single-frame scans and previews keep the film still (FR-014). |
| E3 | Does `advance` move exactly one frame? | `--advance=yes` three times; check consecutive frames, nothing skipped. | Roll scan (FR-002), `advance` command (D5). |
| E4 | What happens at the end of the strip? | Keep scanning with `--advance=yes` past the last frame; record each `scanimage` exit status, `-vv` sense codes, and the image. | End-of-film detection (D4). |
| E5 | What happens with no film? | Eject, then scan once. | The "no film" outcome (D4). |
| E6 | Preview resolution and time | `--preview=yes`; read the image size and time it. | SC-002 and the preview description in the contract. |
| E7 | Are preset resolutions honored? | Scan at 1800, 3600, 5000 dpi (Color 8-bit for speed) and compare pixel sizes with the expected sizes in [data-model.md](data-model.md). | Whether presets need snapping to resolutions the scanner supports. |
| E8 | Time per preset | One RGBI 16-bit frame at each preset, timed. | SC-003, and the speed figures shown by `primefilm presets`. |

If E1–E3 fail (the XA does not understand pieusb's transport commands), roll scanning cannot
be built on pieusb. The fallback is a USB capture of VueScan advancing the film and a
direct-USB backend behind the same interface (constitution Principle III). That would be its
own feature; single-frame scanning and preview would still ship from this one.

## Findings from the hardware (2026-11-09)

Experiments E1–E8 with a 4-frame B&W negative strip; details in
[hardware-validation.md](hardware-validation.md).

- **Slide transport works with flags `0x01`** (E1–E3). `sane.d/pieusb.conf` now ships `0x01`.
  `SLIDE INIT` before every scan leaves a loaded strip where it is; `advance` moves exactly one
  frame, and frames come out correctly positioned.
- **No end-of-film signal** (E4). At the last frame `SLIDE NEXT` silently does nothing: further
  scans return GOOD and show the last frame again. Two scans of the same frame differ by
  0.6–1.7/255 on average (sensor noise); neighbouring frames by 8–17/255.
- **No "no film" signal** (E5). With the scanner empty, the scan succeeds and shows the bare
  light source: channel means about 205/199/206 of 255 (80% of full scale, not saturated),
  uniform apart from dust specks.
- **Preview** (E6) is 300 dpi (431 × 287) and takes 11.8 s, the same as an ordinary 300 dpi
  scan. Every scan spends about 6 s calibrating, so small scans cost about 12 s regardless.
- **Resolutions are honored** (E7): 1800 → 2586 × 1721 (28.9 s), 3600 → 5173 × 3443 (50.0 s),
  5000 → 7185 × 4783 (54.1 s), all 8-bit color. The driver's rounding can give one pixel more
  than `floor(mm / 25.4 × dpi)`; the real size is recorded per scan.
- **Timing**: 19.5 s for the very first scan after connecting (warm-up), then about 12 s.
- **RGBI 16-bit times per frame** (E8): web 49 s, standard 91 s, high 5000 dpi 314 s.
- **The real maximum is 5000 dpi** (E8). The scanner reports 10000 × 10000 dpi, but its sensor
  is 7185 pixels wide (36.5 mm at 5000 dpi). A 10000 dpi scan came out 7185 × 9566 in 16 min:
  only the lengthwise step got finer, so pixels are twice as tall as wide. The `max` preset was
  dropped and `--resolution` is capped at 5000. TIFF resolution tags are now computed per side
  from the pixels delivered, so a file can never claim square pixels it doesn't have.
- **Busy while loading** (validation step 2): right after a strip is inserted, `sane_start`
  returns "device busy" for some seconds while the scanner pulls the film in. Reported as
  "the scanner is not ready (still loading the film?)", not "in use by another program".

## Decisions

### D1. Session-based backend interface

- **Decision**: extend `ScannerBackend` with `open(device) -> Session`, where the session type
  is an associated type (`type Session: ScanSession`). A `ScanSession` can `configure` the
  scan settings, `start_frame` (returns the frame's size and layout), `read` frame data in
  chunks until the end of the frame, `end_frame`, and read back all options. The roll, single,
  preview and advance logic is written once against these traits (in `src/scan.rs`) and runs
  unchanged with the real scanner and the fake one.
- **Ownership change**: the safe SANE `Device` will *own* the `Sane` context instead of
  borrowing it (`Sane::open(self, name)` consumes the context). Struct fields drop in
  declaration order, so the device handle is closed before SANE exits. This removes the
  `Device<'a>` lifetime from feature 001, which otherwise makes a session that holds both
  values impossible to return (a struct cannot borrow from its own field).
- **Alternatives considered**:
  - One trait method per command (`scan_roll`, `preview`, …) implemented in each backend:
    duplicates the sequencing logic in the fake backend, so tests would not cover the real
    sequencing.
  - Closure-based `with_device(|scanner: &mut dyn ...| ...)`: needs trait objects and is harder
    to read than an associated type (Principle VI).
  - Reopening the device for every frame: pieusb resets the USB link on every open (feature 001
    findings), slow and risky mid-roll.

### D2. Enable pieusb's slide transport for the XA

- **Decision**: change the flags field of the XA line in `sane.d/pieusb.conf` from `0x00` to
  the value E1 finds (`0x01` expected, `0x03` if command 0x17 is rejected). The tool then sets
  `advance` explicitly for every scan: false for single frames and previews, true for roll
  frames.
- **Rationale**: it is the only way to make pieusb move film. The file is already ours and
  embedded in the binary, so system SANE and VueScan are unaffected.
- **Effect on feature 001**: `detect` and `capabilities` send no scan commands, so they are
  unchanged; the hardware tests from 001 are re-run once to confirm.

### D3. Archive TIFF written by our own code

- **Decision**: a small, hand-written baseline TIFF writer (`src/tiff.rs`, about 150 lines):
  little-endian (`II`), uncompressed, 8 or 16 bits per sample, 3 samples (RGB) or 4 (RGB plus
  infrared, tagged `ExtraSamples = 0`, "unspecified", the same layout VueScan uses for its
  64-bit RGBI TIFFs). It writes the 8-byte header, streams the pixel data as it arrives from
  the scanner in fixed-size strips, then writes the tag directory at the end and patches its
  offset into the header. Tags include width, height, bits, samples, photometric RGB, strip
  offsets and byte counts, X/Y resolution in dpi, `Software`, `DateTime` and an
  `ImageDescription` naming the roll, frame and preset.
- **Rationale**: archival output is the core of this tool, so the owner should be able to read
  every byte of how it is written (Principle VI). Uncompressed strips have fixed offsets, so
  the file is written in one pass with bounded memory. The image crates' RGBA types would tag
  the infrared channel as transparency, so viewers would show the image as see-through.
- **Verification**: tests decode our files with the `tiff` crate (dev-dependency only), so the
  writer is checked by an independent reader.
- **Alternatives considered**: `tiff` crate as the writer (alpha tagging problem above; core
  logic hidden in a crate); Deflate/LZW compression (saves perhaps 20–40% on 16-bit film
  grain, but needs a compression crate and per-strip offsets. Deferred until disk space is a
  real problem. A standard roll at the default preset is about 5 GB uncompressed).

### D4. Detecting the end of the film and "no film"

The XA reports neither (findings above), so both are recognized from the image.

- **Decision**: while a frame streams to disk, the tool keeps per-channel statistics plus a
  coarse *fingerprint*: the frame's average color in each cell of a 16 × 16 grid. Then:
  1. **Same frame as before** (roll scans and `advance`): if the fingerprint matches the
     previous frame's (mean absolute difference below `SAME_FRAME_DIFF`, a fraction of full
     scale), the film did not move, so the previous frame was the last one. The repeat is
     discarded and the roll ends with *end of film*.
  2. **Empty gate**: a frame where nearly all samples in every color channel are bright (the
     bare light source measured at about 80% of full scale) is not film. As the first frame
     it means *no film*; later in a roll it ends the run as *end of film*. It is discarded.
  3. **`--frames N`** limit on a roll, for when the owner knows the count.
- **Thresholds** (from E4/E5, 8-bit scale): same frame ≤ 1.7/255 per pixel, different frames
  ≥ 8/255; empty gate ≥ 80% bright. The fingerprint averages away most noise, so
  `SAME_FRAME_DIFF = 1%` of full scale sits well clear of both. `NEAR_FULL = 50%` of full
  scale, `EMPTY_FRACTION = 95%` of samples.
- **Known limitation**: two consecutive genuinely blank frames (unexposed, same density) also
  match. The roll then stops early with *end of film* and the summary's resume command
  continues it; the second blank frame is not saved. Accepted: rare, harmless, and visible.
- **Alternatives considered**: a scanner status (none exists); keeping the previous full frame
  in memory to compare (up to 1.1 GB); counting on `--frames` alone (the owner usually doesn't
  know how many frames a strip has).

### D5. Advancing without scanning

- **Decision**: `primefilm advance` makes two throwaway 25 dpi, 8-bit color scans, writing no
  file: the first with `advance` = true (fingerprints the current frame, then moves the film),
  the second with `advance` = false (fingerprints the frame now in the gate). If the two
  fingerprints match, the film was already at its last frame and did not move; the tool says
  so instead of claiming a movement (FR-013). Takes about 25 s.
- **Rationale**: pieusb only moves film at the end of a non-preview scan; there is no SANE call
  for "move only", and the XA gives no feedback when it cannot advance (E4).

### D6. Ctrl-C finishes the current frame

- **Decision**: the first Ctrl-C asks the run to stop *after the frame in progress is saved*
  (the tool prints that it is finishing the frame). A second Ctrl-C exits immediately, as in
  feature 001; the frame in progress stays a hidden `.partial` file and is never renamed.
- **Rationale**: pieusb captures and advances inside `sane_start`, so by the time the tool can
  act, the film has already moved past the frame. Saving it keeps the folder in step with the
  film for resuming (SC-007). Dropping the frame would silently lose a frame from the roll.

### D7. Writing files safely

- **Decision**: each scan is written to `.<name>.partial` in the target folder, flushed and
  synced to disk, then published with `std::fs::hard_link(partial, final)` and the partial
  removed. `hard_link` fails if `final` exists, so an existing scan can never be replaced, even
  by a race (FR-010). `rename` is not used because it silently replaces the target. The
  sidecar is published first, then the TIFF, so a TIFF never exists without its settings
  record.
- **Leftover partials** from a killed process are listed with `-v` and otherwise ignored.

### D8. Names, folders and numbering

- **Decision**: output root `--output DIR`, default `$HOME/Pictures/primefilm`. Each roll is
  a folder `<root>/<roll>/` holding `<roll>-<NNN>.tif` and `<roll>-<NNN>.json`, with a 3-digit
  frame number (half-frame rolls reach 80 frames; fixed width keeps file managers sorting in
  film order). `--roll` defaults to today's local date (`2026-11-09`). The frame number
  defaults to one past the highest number already in the roll folder, so a second single scan
  or a resumed roll continues numbering automatically; `--frame N` (single) or
  `--start-frame N` (roll) overrides it. If a name is already taken (rescanning a frame at
  another preset), `-2`, `-3`… is appended: `<roll>-<NNN>-2.tif`.
- **Previews** go to `<root>/previews/preview-<YYYYMMDD-HHMMSS>.tif`, apart from all roll
  folders (FR-012).
- **Roll names** may contain letters, digits, `-`, `_` and `.`; anything else is a usage error,
  so names are always safe as folder and file names.

### D9. Preview format

- **Decision**: 8-bit RGB TIFF written by the same writer (D3), no infrared, at pieusb's preview
  resolution.
- **Rationale**: Ubuntu's image viewer and every photo tool open TIFF, so no PNG encoder crate
  is needed. Negatives appear as negatives; inversion is derivative processing and out of
  scope.

### D10. Settings record (sidecar JSON)

Each scan's `<name>.json` holds: schema version, tool version, local timestamp with UTC offset,
scanner identity, roll, frame number, preset, requested and actual resolution, color mode, bit
depth, channel order (`R,G,B,I`), pixel size, seconds taken, film movement after the frame,
and a full snapshot of every scanner option value read after the frame (this includes the
exposure, gain and offset values, constitution Principle I). Serialized with the existing
`serde_json`.

### D11. New crates

| Crate | Why | Alternative rejected |
|-------|-----|----------------------|
| `jiff` 0.2 | Local date and time with the system time zone, for default roll names, preview names, the TIFF `DateTime` tag and sidecar timestamps. Small, well-documented, by the author of `regex`. | `chrono`: equally suitable but its time-zone handling needs an extra crate; hand-written date math from the Unix epoch is error-prone and teaches little. |
| `tiff` 0.11 (dev-dependency only) | Independent decoder to check our TIFF writer in tests. | Trusting our own reader: a bug could be symmetric and go unnoticed. |

No new crate for TIFF writing, compression, PNG or progress bars.

### D12. Testing without the scanner

- **FakeBackend** gains a simulated film strip: `--simulate-frames N` (hidden, default 6)
  frames of generated gradient data in RGBI 16-bit or RGB 8-bit, tracking the film position so
  tests can check that single scans and previews do not move it. New scenarios: `no-film`,
  `jam` (scanner error at frame 3), `disconnect` (now also: disconnect while scanning frame 3),
  `interrupt` (Ctrl-C during frame 2), `empty-gate` (frame 4 is bare light source). Like the
  XA, the fake strip stays on its last frame when asked to advance past it, so the roll ends
  by the same-frame check.
- **SANE `test` backend**: the real FFI scan path (`sane_start`, `sane_get_parameters`,
  `sane_read`, `sane_cancel`, option setting) runs against SANE's bundled test scanner, with an
  option profile for that backend (it has no `advance` or RGBI). Each test is its own process.
- **Hardware tests** (`#[ignore]`): single frame, preview and a short roll, run by the owner.

## Open risks

- **R1, transport (E1–E3)**: resolved; pieusb moves the XA's film with flags `0x01`.
- **R2, USB disconnects mid-roll**: feature 001 R3. A roll at the default preset takes many
  minutes, so the run must stop cleanly and resume (built in), but frequent drops would make
  long rolls tedious. Possibly related to how soon the scanner is opened after it enumerates;
  keeping the device open for the whole roll may help.
- **R3, `/tmp` size**: pieusb buffers each frame in `/tmp`; at `high` that is 275 MB per frame.
  On this machine `/tmp` is a 7.5 GB tmpfs (checked 2026-11-09), so the frame briefly holds
  that much RAM; fine, but worth knowing.
- **R4, preset resolutions (E7)**: resolved; resolutions are honored (one pixel of rounding).
- **R5, color negatives**: the empty-gate threshold was measured with B&W negatives and an
  empty gate. A color negative's orange base is denser than the bare light, but this should be
  checked with a color strip during validation.
