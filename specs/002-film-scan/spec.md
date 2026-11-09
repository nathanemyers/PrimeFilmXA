# Feature Specification: Film Scanning (Single Frame, Roll, Preview)

**Feature Branch**: `002-film-scan`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "Implement the ability to pull scans off of film. This scanner has
the ability to automatically move on and pull scans from a roll of film that is very important,
but it should also be able to scan just one image. I also want the ability to do a quick preview
scan at a low resolution and a series of full scans at different resolutions / speed. I normally
do not need full resolution scans."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Scan a whole strip or roll unattended (Priority: P1)

The owner feeds a film strip or uncut roll into the PrimeFilm XA, starts one command with a
roll name and a quality preset, and walks away. The scanner scans the frame in front of it,
advances to the next frame, scans it, and keeps going until the film runs out. Each frame is
saved as its own numbered file in one folder for that roll, and the owner sees progress
(frame number, elapsed time) as it goes.

**Why this priority**: the owner called automatic advance "very important". Scanning rolls
frame by frame by hand is the slow, error-prone job this tool exists to remove.

**Independent Test**: load a strip of 4–6 frames, run a roll scan at the fastest full-scan
preset, and confirm one file per frame appears, numbered in film order, with no frame skipped
or duplicated, and that the tool reports the end of the film and exits successfully.

**Acceptance Scenarios**:

1. **Given** a strip of N frames is loaded, **When** the owner starts a roll scan, **Then** the
   tool scans and saves N frames in film order, numbered 1 to N, reports the end of the film,
   and exits successfully.
2. **Given** a roll scan is running, **When** each frame finishes, **Then** the tool reports the
   frame number, the file written, and the running elapsed time.
3. **Given** a roll scan is running, **When** the owner presses Ctrl-C, **Then** the tool stops
   after making sure no incomplete file is left looking complete, keeps every finished frame,
   reports how many frames were saved, and releases the scanner.
4. **Given** a roll scan stopped early (interrupted, disconnected, error), **When** the owner
   reloads the film at the next unscanned frame and starts again with the same roll name and a
   starting frame number, **Then** numbering continues from that number and no earlier file is
   overwritten.

---

### User Story 2 - Scan a single frame (Priority: P1)

The owner positions one frame (or loads a single mounted slide or short strip) and scans just
that frame. The film does not advance afterwards, so the owner can rescan the same frame at a
different preset if they want.

**Why this priority**: the simplest end-to-end scan; it is also the building block the roll
scan repeats, and the quickest way to verify the whole scan path on real hardware.

**Independent Test**: load a strip, run a single-frame scan, confirm exactly one file is saved
and that the film has not moved (a second single-frame scan produces the same image).

**Acceptance Scenarios**:

1. **Given** film is loaded, **When** the owner runs a single-frame scan, **Then** exactly one
   frame is scanned and saved, and the film stays where it was.
2. **Given** a file with the target name already exists, **When** the owner scans, **Then** the
   existing file is left untouched and the new scan is saved under a different, clearly related
   name (or the tool refuses with a clear message), never overwriting.
3. **Given** no film is loaded, **When** the owner runs a single-frame scan, **Then** the tool
   reports that no film was found, writes no file, and exits with a failure status.

---

### User Story 3 - Quick low-resolution preview (Priority: P2)

Before committing to a slower full scan, the owner runs a quick preview of the current frame
at low resolution to check that film is loaded, the frame is framed correctly, and which way
up it is. The preview is saved as an ordinary image file the owner can open in any viewer.

**Why this priority**: saves time and wasted full scans, but full scanning is useful without it.

**Independent Test**: load film, run a preview, confirm it completes much faster than the
fastest full scan, produces an image that opens in a standard image viewer, and does not move
the film.

**Acceptance Scenarios**:

1. **Given** film is loaded, **When** the owner runs a preview, **Then** a low-resolution image
   of the current frame is saved within the preview time target, and the film does not advance.
2. **Given** a preview was just taken, **When** the owner runs a single-frame full scan,
   **Then** the full scan captures the same frame that was previewed.
3. **Given** previews are made, **When** the owner looks at the output, **Then** previews are
   clearly distinguishable from archival scans by name and location, so they are never mistaken
   for archive masters.

Preview works only on the frame currently in the scanner. To survey a roll, the owner
previews, advances (User Story 5), and repeats. Previewing a whole roll and then choosing frames
is out of scope.

---

### User Story 4 - Choose a quality preset (Priority: P2)

The owner picks one of a small, named set of full-scan presets that trade resolution for speed
and file size. Because the owner normally does not need full resolution, the default preset is
a mid-resolution one; the full optical resolution is available but must be asked for.

**Why this priority**: the owner explicitly wants several resolution/speed levels and usually
not the maximum. Single and roll scans work with one fixed preset, so this layers on top.

**Independent Test**: scan the same frame with each preset and confirm each file's pixel
dimensions match the preset's resolution, that scan time increases with resolution, and that
the recorded settings name the preset used.

**Acceptance Scenarios**:

1. **Given** the owner does not name a preset, **When** they scan, **Then** the default
   (mid-resolution) preset is used and named in the output and metadata.
2. **Given** the owner lists the presets, **When** the list is shown, **Then** each preset shows
   its name, resolution, approximate image size in pixels, approximate file size, and a relative
   speed indication.
3. **Given** the owner names an unknown preset, **When** they run a scan, **Then** the tool
   lists the valid presets and exits with a usage error before touching the scanner or film.

The presets are "web" (about 1800 dpi), "standard" (about 3600 dpi, the default), "high"
(about 5000 dpi, the most the scanner resolves). A "max" 10000 dpi preset was dropped after
hardware testing: the scanner's sensor resolves at most 5000 dpi across the frame, so 10000 dpi
gives stretched images. Presets change resolution
only: every full scan keeps full bit depth and the infrared channel.

---

### User Story 5 - Advance film without scanning (Priority: P3)

The owner can explicitly advance the film by one frame without scanning, for example to skip a
damaged frame or to step through a strip while previewing. Ejecting is done with the scanner's
own eject button, not by this tool.

**Why this priority**: convenient around single-frame work and previews, but repositioning the
film by hand covers it in the meantime.

**Independent Test**: load a strip, preview, advance once, preview again, and confirm the second
preview shows the next frame.

**Acceptance Scenarios**:

1. **Given** film is loaded, **When** the owner asks to advance, **Then** the film moves forward
   exactly one frame, the movement is logged, and no scan file is saved.

---

### Edge Cases

- The film runs out partway through a roll scan: this is the normal end, not an error.
- The scanner drops off USB mid-scan or mid-roll (observed on this hardware about 90 s after
  connecting): the in-progress frame is discarded, finished frames are kept, the tool reports
  the last frame saved and how to resume, and does not retry in a loop.
- The scanner is busy (another program holds it) or not found when a scan starts: same
  outcomes and exit statuses as detection (feature 001).
- Film is jammed or the scanner reports a mechanical error: scanning stops, the error is shown,
  and the film is not moved further.
- A frame is blank (unexposed or end leader): it is still scanned and saved so frame numbering
  matches the physical film.
- Frame spacing is irregular (half frames, panoramic, cut strips with uneven edges): the
  scanner's own frame positioning is used; misaligned frames are the scanner's limitation and
  can be corrected by rescanning that frame on its own.
- The output disk fills up mid-roll: the incomplete frame is discarded, the run stops with a
  clear message, finished frames are kept.
- The output folder for the roll name already contains frames: existing files are never
  overwritten.
- Ctrl-C arrives while the film is moving: the movement finishes or stops safely before the
  scanner is released.

## Requirements *(mandatory)*

### Functional Requirements

**Scanning**

- **FR-001**: The owner MUST be able to scan the single frame currently in the scanner without
  the film advancing afterwards.
- **FR-002**: The owner MUST be able to scan a strip or roll in one command: scan the current
  frame, advance one frame, and repeat until the scanner reports the end of the film.
- **FR-003**: A roll scan MUST accept a starting frame number so an interrupted roll can be
  resumed with continuous numbering.
- **FR-004**: The owner MUST be able to make a quick, low-resolution preview of the frame
  currently in the scanner without advancing the film. Previewing a whole roll is out of scope.
- **FR-005**: The tool MUST offer these named full-scan quality presets, which differ only in
  resolution (and therefore speed and file size): `web` (~1800 dpi), `standard` (~3600 dpi,
  the default) and `high` (~5000 dpi, the scanner's real maximum). The owner
  MUST be able to list them.
- **FR-006**: The owner MAY override a preset's resolution with an explicit value within the
  scanner's supported range.

**Output and archival fidelity**

- **FR-007**: Every full scan MUST be saved losslessly at the scanner's full captured bit depth,
  including the infrared channel, regardless of the preset's resolution.
- **FR-008**: Every saved scan MUST record the settings used to capture it (preset, resolution,
  bit depth, color mode, exposure, scanner identity, roll name, frame number, timestamp).
- **FR-009**: Scans from one roll MUST be saved in one folder named after the roll, with file
  names that sort in film order and contain the roll name and a zero-padded frame number.
- **FR-010**: The tool MUST NEVER overwrite an existing scan file.
- **FR-011**: A scan that fails or is interrupted MUST NOT leave a file that looks complete.
- **FR-012**: Previews MUST be saved in a common viewable image format, clearly separated from
  archival scans by name and location.

**Film handling and safety**

- **FR-013**: Every film movement MUST happen only because of an explicit command (a roll scan
  or an advance) and MUST be logged.
- **FR-014**: Single-frame scans and previews MUST NOT move the film to a different frame.
- **FR-015**: The owner MUST be able to advance the film one frame without scanning. Ejecting
  film is out of scope: the owner uses the scanner's physical eject button.
- **FR-016**: The tool MUST release the scanner on every exit path, including errors, Ctrl-C
  and disconnects.
- **FR-017**: A USB disconnect, jam or scanner error during a scan MUST stop the run with a
  clear message naming the last frame saved; the tool MUST NOT silently retry.

**Interface**

- **FR-018**: All scanning functions MUST be usable from the command line with non-interactive
  arguments, never prompting, with distinct non-zero exit statuses for failure outcomes
  (reusing feature 001's outcomes where they apply, plus "no film" and "scan failed").
- **FR-019**: Long operations MUST report progress (current frame, elapsed time) on the
  terminal, and a summary at the end (frames saved, total time, output folder).
- **FR-020**: Machine-readable output (as in feature 001) MUST be available for the result of
  every scan command, listing each file written.
- **FR-021**: All scanning logic other than the hardware itself (sequencing, naming, metadata,
  presets, error and interrupt handling) MUST be exercisable without the physical scanner,
  using a simulated scanner that can produce frames, the end of film, disconnects and jams.

### Key Entities

- **Roll**: one physical strip or roll being scanned; has a name chosen by the owner, an output
  folder, and the frames scanned from it.
- **Frame**: one image position on the film; has a frame number in film order and zero or more
  scans and previews.
- **Scan**: one captured image of one frame; has the preset, the exact settings used, the
  timestamp, the scanner identity, and the file it was saved to.
- **Preview**: a low-resolution, non-archival image of the current frame, kept apart from scans.
- **Quality Preset**: a named combination of resolution and related settings, with expected
  pixel size, file size and relative speed.
- **Scan Run Outcome**: the result of a single, roll or preview run: completed (with frames
  saved), end of film, interrupted, no film, disconnected, scan failed, plus the detection
  outcomes from feature 001; each maps to a message, next step and exit status.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A strip of 6 frames is scanned unattended with one command, producing exactly 6
  correctly numbered files with no skipped or duplicated frames, in 10 of 10 trial runs.
- **SC-002**: A preview of one frame completes in under 15 seconds from command to viewable
  file.
- **SC-003**: At the default preset, a full scan of one frame takes no more than half the time
  of a scan at the maximum-resolution preset.
- **SC-004**: In 100% of interrupted, disconnected or failed runs, every file that exists is a
  complete, valid scan, and every completed frame up to the failure is kept.
- **SC-005**: In 100% of runs, no existing scan file is overwritten.
- **SC-006**: After any run, including failed and interrupted ones, another scanning program
  can open the scanner immediately.
- **SC-007**: An interrupted roll can be resumed so the final folder holds one file per
  physical frame with continuous numbering.
- **SC-008**: All non-hardware behavior is verified by automated checks that pass with no
  scanner attached.

## Clarifications

### Session 2026-11-09

- Q: Should preview cover only the current frame, or a whole roll with frames picked
  afterwards? → A: Current frame only; roll preview is out of scope.
- Q: Which presets, and which default? → A: web ~1800, standard ~3600 (default), high ~5000,
  max 10000 dpi. (Revised 2026-11-09 after hardware testing: `max` dropped because the
  scanner resolves at most 5000 dpi across the frame and 10000 dpi gives stretched images;
  `high` is the maximum.)
- Q: May faster presets drop the infrared pass to save time? → A: No. Presets change
  resolution only; infrared and full bit depth are always captured.
- Q: Does the tool need to eject film? → A: No. The owner uses the scanner's physical eject
  button (stated at `/speckit-plan`).

## Assumptions

- The scanner positions each frame itself (it detects frame gaps and advances one frame at a
  time); the tool relies on that and does not do its own frame detection. This must be
  confirmed on hardware, along with how the scanner signals "end of film".
- Film-type processing (negative inversion, color correction, dust removal, cropping) produces
  derivative files and is out of scope; this feature saves raw scans only. Scans of negatives
  will look like negatives.
- Presets change resolution only; full bit depth and the infrared channel are always captured
  for full scans (constitution Principle I). Previews are not archival and may use reduced bit
  depth, no infrared, and a viewable format.
- Scans are saved under a default output folder in the owner's home directory unless another
  folder is given on the command line.
- Any batch beyond one roll per command (for example several rolls queued) is out of scope.
- The scanner's known habit of dropping off USB (feature 001 research, R3) may interrupt long
  roll scans; this feature handles it safely and supports resuming, but finding its cause is
  separate work.
- The detection, busy and permission checks from feature 001 run before every scan.
