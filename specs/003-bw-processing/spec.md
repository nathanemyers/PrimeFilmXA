# Feature Specification: Black-and-White Negative Processing

**Feature Branch**: `003-bw-processing`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "add black and white post processing. I use two types of film, HP5
and FP4, although the difference in terms of post processing should be minimal if any. I will
want to reverse the negative so that it looks normal. Later I will add post processing for
color film which may be more complicated."

**Revision (2026-11-09, after the first real scans)**: some processed frames were ruined
(outlines, flat patches, clipped highlights). The same frames look the same in VueScan, so the
damage is on the negatives (research P17). Dust removal is off by default until it has been
tested on real captures. Positives
keep the automatic per-frame black and white points, mapped in density, but there is no
brightness, contrast, explicit points or tone matching: tones are finished in post. Each frame
is one TIFF; the JPEG share copy and the JSON sidecar are dropped, and the settings record is
embedded in the TIFF. User Stories 4 and 5 are withdrawn. The requirements below are the
revised ones; see Clarifications.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Scan a B&W frame straight to a positive (Priority: P1)

The owner loads a black-and-white negative, names the film it was shot on (HP5 or FP4), and
scans a single frame. Instead of a raw negative, they get a 16-bit grayscale positive: tones
reversed, no color tint, nothing clipped or stretched, ready for levels and curves in post
*(revised; originally a finished, tone-balanced picture plus a share copy)*. The raw negative
is never kept.

**Why this priority**: this is the whole point of the feature. Every scan of a negative
otherwise needs manual work in another program before it can be looked at or shared.

**Independent Test**: load a B&W strip, scan one frame naming the stock, and confirm the two
positive TIFFs appear, open in an ordinary image viewer as reversed images of the frame, and no
raw negative file is left on disk.

**Acceptance Scenarios**:

1. **Given** a B&W negative is loaded, **When** the owner scans a single frame, **Then** a
   full-quality grayscale positive of that frame is saved, dark areas of the negative
   rendered light and light areas dark, and no raw scan is saved.
2. **Given** a positive is saved, **When** the owner inspects it, **Then** it uses close to the
   full tonal range (deep blacks and clean whites without large areas clipped) and shows no tint.
3. **Given** a positive is saved, **When** the owner reads the settings recorded inside it,
   **Then** they include the film stock, the capture settings (preset, resolution, scanner,
   roll, frame, timestamp) and every processing setting used, including the automatically
   chosen black and white points.
4. **Given** a positive for that frame already exists, **When** the owner scans the frame again,
   **Then** the existing files are left untouched and the new ones are saved under clearly
   related names (feature 002's no-overwrite rule).

---

### User Story 2 - Scan a whole B&W roll straight to positives (Priority: P1)

The owner starts an unattended roll scan (feature 002, User Story 1) naming the film stock.
Each frame is turned into a positive and share copy as soon as it is captured, so the roll
folder fills with finished pictures in film order and no raw negatives.

**Why this priority**: the owner scans whole rolls; positives that need a second pass would undo
the time unattended scanning saves.

**Independent Test**: scan a 4–6 frame strip as a roll and confirm one positive and one share
copy per frame, numbered in film order, with no raw negatives left on disk.

**Acceptance Scenarios**:

1. **Given** a strip of N frames, **When** the owner runs a roll scan, **Then** N positives and
   N share copies are saved, numbered 1 to N in film order.
2. **Given** a roll scan is running, **When** each frame finishes, **Then** progress shows the
   frame number, the files written and the elapsed time.
3. **Given** a roll scan stops early (Ctrl-C, disconnect, error), **When** the owner resumes
   with the same roll name and next frame number, **Then** numbering continues and no earlier
   file is overwritten (feature 002 behavior, unchanged).
4. **Given** one frame cannot be processed after it was captured, **When** this happens, **Then**
   the run stops like any other scan failure, names the frame to rescan, and keeps every
   finished frame.

---

### User Story 3 - Remove dust and specks (Priority: P2)

*Revised 2026-11-09: off by default until tested and tuned on real captures; available with
`--dust` (research P17).*

Dust and lint on the film block light, so they show up as small white specks and hairs on the
positive. The owner wants these cleaned up automatically, without the picture being softened or
real small bright details (stars, catchlights, street lights) being erased.

**Why this priority**: dust is on almost every frame and spotting it by hand is the slowest part
of finishing a scan, but an uncleaned positive is still usable.

**Independent Test**: scan the same dusty frame with dust removal on and off; confirm the specks
are gone in the cleaned positive, everything away from the specks is identical in both, and the
settings record that dust removal was applied.

**Acceptance Scenarios**:

1. **Given** a frame with dust specks, **When** the owner scans it, **Then** dust removal is
   applied by default and the specks are filled in to match their surroundings.
2. **Given** dust removal is turned off, **When** the same frame is scanned, **Then** the
   positive matches the cleaned one everywhere except at the specks.
3. **Given** a frame with small genuine bright details, **When** dust removal runs at the
   default strength, **Then** those details are kept.
4. **Given** dust removal ran, **When** the owner reads the recorded settings, **Then** they show
   it was applied, its strength, and how many specks were removed.

---

### User Story 4 - Adjust the tones of a frame (Priority: P2) — WITHDRAWN 2026-11-09

*Withdrawn: the owner sets tones in post-processing. Kept for the record.*

Most frames come out well automatically, but some (a dark interior, a lot of sky, a badly
exposed frame) need a nudge. Because the raw negative is not kept, the owner rescans that frame
with brighter/darker or more/less contrasty settings.

**Why this priority**: automatic tones cannot suit every frame, but the automatic result is
usable without this.

**Independent Test**: scan one frame with default settings, then with a brighter and with a more
contrasty setting; confirm each result differs in the expected direction and records its
settings.

**Acceptance Scenarios**:

1. **Given** a frame, **When** the owner scans it asking for a brighter result, **Then** midtones
   are lighter than in the default result while black and white stay anchored.
2. **Given** a frame, **When** the owner asks for more (or less) contrast, **Then** the tonal
   spread increases (or decreases) compared with the default result.
3. **Given** an adjustment outside the allowed range, **When** the owner runs the command,
   **Then** the tool reports the valid range and exits with a usage error before touching the
   scanner or film.

---

### User Story 5 - Matching tones across a roll (Priority: P3) — WITHDRAWN 2026-11-09

*Withdrawn: the owner sets tones in post-processing. Kept for the record.*

When frames on a roll were shot in the same light (a series, portraits in one session), the
owner wants them to match rather than each being auto-adjusted on its own. Per-frame tones stay
the default; matching is an option on the roll scan.

**Why this priority**: refinement for series work; per-frame results are already usable.

**Independent Test**: scan a strip with matching on and confirm every positive records the same
black and white points, and two frames of the same scene look equally bright.

**Acceptance Scenarios**:

1. **Given** matching is on, **When** a roll scan runs, **Then** the tones are set from the first
   frame that is not blank and every later frame uses the same black point, white point and
   adjustments, recorded on each.
2. **Given** the owner supplies explicit tone values for the roll, **When** the roll scan runs,
   **Then** every frame uses those values.
3. **Given** a matched roll scan was interrupted, **When** the owner resumes it, **Then** the
   owner can pass the values recorded on the last saved frame so the rest of the roll matches.

---

### User Story 6 - Positive previews (Priority: P3)

When the owner previews a B&W frame (feature 002, User Story 3), the preview is shown as a
positive too, so framing and exposure can be judged on a normal-looking image.

**Why this priority**: convenient; a negative preview still shows framing.

**Independent Test**: preview a B&W frame naming the stock and confirm the preview image is a
positive and is still kept apart from archive files.

**Acceptance Scenarios**:

1. **Given** B&W film is loaded, **When** the owner previews, **Then** the preview is a positive,
   saved in the preview location within the preview time target.

---

### User Story 7 - Preview a whole roll (Priority: P3) — added 2026-11-09

The owner runs through a whole strip at preview speed to see what is on it before committing
to full scans. Each frame is scanned at preview resolution and the film advances, as in a roll
scan. *(Feature 002 left roll previews out of scope; the owner asked for this.)*

**Independent Test**: run `roll --preview` on a 4–6 frame strip and confirm one 8-bit preview
per frame (both inversions) in `previews/<roll>/`, in film order, at about the single-preview
time per frame, and nothing in the roll's archive folder.

**Acceptance Scenarios**:

1. **Given** a strip of N frames, **When** the owner runs a roll preview, **Then** N previews
   are saved in `previews/<roll>/`, numbered 1 to N, and the run ends at the end of the film.
2. **Given** a roll preview stops early, **When** the owner runs the printed resume command,
   **Then** it continues as a roll preview.
3. **Given** a quality preset, resolution or dust level is given with `--preview`, **Then** the
   tool exits with a usage error before touching the scanner.

---

### Edge Cases

- The scan includes clear film base or frame gap at the edges: these become black in the
  positive and MUST NOT distort automatic tones for the picture itself.
- A blank frame (unexposed, end leader) has almost no tonal range: it is still saved so
  numbering matches the film, without stretching grain into a full-contrast image, and is
  reported as "probably blank". Matching tones (User Story 5) never take their values from it.
- A fully fogged or overexposed frame (almost uniformly dense): handled the same way.
- A small very bright or very dark area (specular highlight, light source) must not set the
  whole frame's white or black point by itself.
- Dust lying across fine detail is filled in from its surroundings as well as possible; a long
  scratch across the frame may only be partly removed.
- Many genuine small bright points (night sky, sparkling water): dust removal errs on keeping
  detail; the owner can lower its strength or turn it off.
- The owner names a color film stock: the tool says color film is not supported yet and exits
  with a usage error before touching the scanner or film.
- The owner names an unknown film stock: the tool lists the supported stocks and exits with a
  usage error.
- Processing fails after the frame was captured (out of memory, internal error): no partial
  positive is left, the capture is lost, and the frame is named for rescanning.
- The disk fills up while writing: the incomplete files are discarded, the run stops, finished
  frames are kept.
- A frame's processing result is disappointing after the fact: the only remedy is rescanning
  that frame with different settings; this is accepted because raw negatives are not kept.

## Requirements *(mandatory)*

### Functional Requirements

*Revised 2026-11-09. Former FR-005 and FR-006 (brightness and contrast, explicit points, tone
matching) and FR-016 (share copy) are withdrawn.*

**Conversion**

- **FR-001**: Single-frame scans, roll scans and previews of B&W film MUST produce processed
  positives as their output, processing each frame as soon as it is captured.
- **FR-002**: The positive MUST be grayscale with tones reversed: clear film base renders black,
  the densest parts of the negative render white, density mapped evenly in between.
- **FR-003**: The conversion MUST produce neutral gray with no tint, combining the color
  channels; the infrared channel is not used for the image.
- **FR-004**: Black and white points MUST be set automatically per frame from the picture area,
  ignoring film base and frame gap at the edges, and not letting very small extreme areas
  decide them alone. A frame with almost no tonal range is flagged "probably blank" and not
  stretched. There are no other tone adjustments.
- **FR-007**: The same captured image processed with the same settings MUST produce identical
  positives every time.

**Dust removal**

- **FR-008**: Processing MUST be able to remove dust specks and hairs, filling each with tones
  matched to its surroundings and leaving every other pixel unchanged. It is off by default.
  For B&W film it works from the image itself, because the infrared channel shows the silver
  image as well as the dust.
- **FR-009**: The owner MUST be able to choose dust removal's strength from a small documented
  range (off, gentle, normal, strong).
- **FR-010**: Dust removal MUST NOT blur or soften areas without dust, and at `normal` MUST
  keep small genuine bright details such as stars and catchlights.

**Film stocks and types**

- **FR-011**: The owner MUST be able to name the film stock: `hp5` (Ilford HP5 Plus), `fp4`
  (Ilford FP4 Plus), or a generic black-and-white choice for any other B&W negative, which is
  the default. The owner MUST be able to list the supported stocks.
- **FR-012**: Each stock MUST have its own processing profile, even though HP5 and FP4 start out
  identical, so one can be tuned later without affecting the others.
- **FR-013**: Processing MUST be organized by film type (B&W negative now) so a color negative
  type can be added later without changing B&W behavior. Naming a color stock today MUST give a
  clear "not supported yet" usage error.

**Output and archival fidelity**

- **FR-014**: Raw negative scans MUST NOT be saved. Any temporary data used while processing a
  frame MUST be removed when the frame is done or the run ends, on every exit path.
- **FR-015**: Each positive MUST be full resolution, lossless, grayscale, with at least 16 bits
  per pixel; the linear one is the archive master.
- **FR-017**: Every positive MUST carry, embedded in the file, the capture settings (preset,
  resolution, bit depth, scanner identity, roll name, frame number, timestamp, exposure and
  every other scanner option), the film stock, the inversion, every processing setting used,
  and the tool version. No sidecar files are written.
- **FR-018**: Files MUST follow feature 002's naming, folder, no-overwrite and
  never-look-complete-when-incomplete rules. A frame counts as saved only when both its
  positives are complete; the master is published last.
- **FR-019**: Previews MUST stay separate from archive files (feature 002, FR-012).

**Interface**

- **FR-020**: Film stock and dust removal settings MUST be non-interactive command-line options
  on the scan commands, validated before the scanner or film is touched.
- **FR-021**: Progress, end-of-run summary and machine-readable output (feature 002, FR-019 and
  FR-020) MUST list both files written per frame.
- **FR-023**: `roll --preview` MUST scan every frame of a roll at preview resolution (300 dpi,
  8-bit), advancing the film like a roll scan, and save the previews in `previews/<roll>/`,
  apart from archival scans. It MUST reject `--preset`, `--resolution` and `--dust`.
- **FR-022**: All processing (inversion, dust removal, film-stock handling) MUST be verifiable
  by automated checks using synthetic or recorded captures, with no scanner attached.

### Key Entities

- **Film Type**: a family of films processed the same general way: B&W negative now, color
  negative later.
- **Film Stock**: a named film (HP5, FP4, generic B&W) belonging to a film type, with a
  processing profile.
- **Processing Profile**: a stock's default settings (automatic tone method, default brightness,
  contrast and dust removal strength).
- **Processing Settings**: the settings actually used for one frame: stock, black and white
  points (automatic, matched or explicit), brightness, contrast, dust removal and strength.
- **Positive Master**: the lossless 16-bit grayscale archive image of one frame, with capture
  and processing settings recorded.
- **Share Copy**: the 8-bit compressed copy of the same frame, for viewing and sharing.

## Success Criteria *(mandatory)*

### Measurable Outcomes

*Revised 2026-11-09: former SC-008 (matched tones) is withdrawn.*

- **SC-001**: On the owner's own HP5 and FP4 rolls, with dust removal off, no frame shows an
  artifact the same frame does not show in VueScan, and at least 9 out of 10 frames need no
  more than ordinary finishing in post.

- **SC-002**: At the default preset, a roll scan with processing takes no more than 20% longer
  than the same roll scan took without processing in feature 002.
- **SC-003**: At the default preset, a 36-frame roll uses no more than one third of the disk
  space its raw scans used in feature 002.
- **SC-004**: A synthetic negative step wedge becomes a positive with the steps in reversed
  order, every step distinguishable, the darkest near pure black and the lightest near pure
  white.
- **SC-005**: On a synthetic frame with 50 added dust specks, at least 95% are removed and no
  pixel more than a few pixels from a speck is changed; on a synthetic star field, at least 95%
  of the stars survive at `normal`.
- **SC-006**: Once tuned (research E11), dust removal leaves no visible artifacts on the
  owner's own HP5 and FP4 frames.
- **SC-007**: After any run, including failed and interrupted ones, no raw negative or temporary
  file is left on disk, and every positive that exists is complete.
- **SC-009**: All processing behavior is verified by automated checks that pass with no scanner
  attached.

## Dependencies

- **Constitution amendment (done 2026-11-09, v2.0.0).** Principle I required raw scans to be
  saved losslessly at full bit depth with the infrared channel. This feature deliberately
  replaces raw scans with processed positives, at the owner's request. Principle I now allows
  a lossless, at least 16-bit processed master for film types whose spec says so.
- Builds on feature 002's scan, roll, preview, naming, no-overwrite and resume behavior; it
  changes what those commands save.

## Clarifications

### Session 2026-11-09

- Q: Keep dust removal? → A: Yes. For B&W film it is image-based: the infrared channel holds
  the full silver image (feature 002 hardware test 3), so infrared dust removal does not work on
  HP5 or FP4. Infrared dust removal is reserved for color film.
- Q: What happens to raw negatives? → A: They are never saved; the owner only wants the
  processed image. A disappointing result is fixed by rescanning the frame.
- Q: When does processing run? → A: During scanning only; there is no separate command to
  process saved files.
- Q: Tones per frame or per roll? → A: Per frame by default, with an option to match tones
  across a roll.
- Q: Share-ready copy? → A: Always save an 8-bit compressed copy alongside the 16-bit master.
  *(Superseded below.)*

### Session 2026-11-09 (after the first real scans)

- Q: Some real frames were ruined (outlines, flat patches, clipped highlights). What changes?
  → A: Remove all tone balancing; tones are set in post. Drop the JSON sidecar and the JPEG;
  keep only TIFFs. (The settings record moves inside each TIFF, as constitution Principle I
  requires a record.)
- Q: What about dust removal, the likely cause of the outlines (dense highlights taken for
  dust)? → A: Off by default until tuned on real captures; still available with `--dust`.
- Q: How should the negative be reversed without tone balancing? → A: Write both a linear and
  a density inversion for every frame for now; the owner will pick one.
- Q: Both fixed inversions (linear, density) looked bad on a real roll preview; the old
  processing worked better. → A: Restore the automatic black and white points as the only
  output, with dust removal still off by default (research P16). Brightness, contrast,
  explicit points and tone matching stay removed.
- Q: Were the artifacts caused by the tool? → A: No. The owner saw the same outlines and flat
  patches in VueScan, so they are on the developed negatives (research P17).
- Q: Can a whole roll be previewed? → A: Yes, `roll --preview` (User Story 7, FR-023). The
  scanner's preview mode cannot move film, so each frame is an ordinary 300 dpi scan, which
  002 experiment E6 measured at the same ~12 s as a preview.

## Assumptions

- HP5 and FP4 start with the same processing profile; any difference is a later tuning change.
- Tone matching takes its values from the first non-blank frame of the run (or explicit values)
  rather than from the whole roll, so frames can be finished as they are scanned without
  holding the whole roll's raw data.
- Dust removal targets specks and short hairs; long scratches and large defects may need hand
  retouching.
- Cropping, rotation, straightening, sharpening, grain reduction and local (dodge/burn) edits
  are out of scope; positives keep the scan's full framing.
- Color negative processing (orange mask removal, per-channel balance, infrared dust removal) is
  a later feature. Until then, the tool cannot produce useful output for color film.
- Chromogenic B&W films (for example Ilford XP2, developed in color chemistry) are not covered
  by the generic B&W choice and wait for the color feature.
- Because raws are not kept, later improvements to a stock's profile do not apply to frames
  already scanned; they would need rescanning. The owner accepts this.
- Recorded captures may still be kept deliberately as test fixtures (constitution workflow
  section); that is development data, not normal output.
