# Feature Specification: Scanner Detection and Capability Listing

**Feature Branch**: `001-scanner-detect`

**Created**: 2026-11-08

**Status**: Draft

**Input**: User description: "A Rust CLI that finds the PrimeFilm XA through SANE and lists its
capabilities."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Check that the scanner is ready (Priority: P1)

Before loading film, the owner runs one command to find out whether the PrimeFilm XA is
connected, powered on and usable. The tool answers with a clear yes, naming the scanner it
found, or a clear no.

**Why this priority**: every later feature (preview, scan, batch) depends on reliably finding
the scanner. The scanner has already been seen dropping off USB, so a fast "is it there?" check
has immediate value on its own.

**Independent Test**: with the scanner on, run the command and confirm it reports the scanner's
identity; turn the scanner off, run it again and confirm it reports that no scanner was found.

**Acceptance Scenarios**:

1. **Given** the scanner is connected and powered on, **When** the owner runs the detect
   command, **Then** the tool reports that a PrimeFilm XA was found, with its model name and
   connection identifier, and exits successfully.
2. **Given** the scanner is unplugged or powered off, **When** the owner runs the detect
   command, **Then** the tool reports that no PrimeFilm XA was found, suggests checking power
   and cable, and exits with a failure status.
3. **Given** other imaging devices are attached (such as a webcam or capture card), **When** the
   owner runs the detect command, **Then** those devices are not reported as scanners.

---

### User Story 2 - See what the scanner can do (Priority: P2)

The owner asks the tool to list the scanner's capabilities: supported resolutions, color modes,
bit depths, scan area, infrared (dust-removal) support, and every other setting the scanner
exposes, with current values and allowed ranges.

**Why this priority**: knowing exactly what the scanner offers drives every later decision about
scan quality and archive format, and confirms whether the open driver path is good enough.

**Independent Test**: with the scanner on, run the capabilities command and compare the list
against the same scanner's settings as shown by another tool (such as the probe script); every
setting must appear with matching values.

**Acceptance Scenarios**:

1. **Given** the scanner is detected, **When** the owner runs the capabilities command,
   **Then** the tool shows a short summary of the key film-scanning capabilities (resolutions,
   color modes, bit depths, scan area, infrared support) followed by the full list of settings.
2. **Given** the scanner is detected, **When** the owner requests machine-readable output,
   **Then** the same information is printed in a structured format that other programs and
   scripts can parse.
3. **Given** a setting is currently inactive or read-only, **When** it is listed, **Then** it is
   marked as such rather than hidden.

---

### User Story 3 - Understand why detection failed (Priority: P3)

When the scanner cannot be used, the owner gets a specific explanation and a next step instead
of a generic error: the scanner is off, another program is using it, the user lacks permission,
the scanner driver is missing, or the scanner disconnected while being queried.

**Why this priority**: today's first attempts hit "Device busy" and silent disconnects; specific
diagnoses save time, but the tool is still useful without them.

**Independent Test**: reproduce each failure condition (scanner off; another scanning program
holding the scanner; scanner unplugged mid-query) and confirm each yields a different message
and exit status.

**Acceptance Scenarios**:

1. **Given** another program is holding the scanner, **When** the owner runs any command,
   **Then** the tool says the scanner is busy, suggests closing other scanning programs, and
   exits with a "busy" status distinct from "not found".
2. **Given** the scanner disconnects while capabilities are being read, **When** this happens,
   **Then** the tool reports the disconnect, does not retry endlessly, and exits with a failure
   status.
3. **Given** the owner asks for verbose diagnostics, **When** a command runs, **Then** the tool
   additionally shows the low-level steps it took, to help investigate problems.

---

### Edge Cases

- The scanner is powered on but has not finished starting up when the command runs.
- The scanner disconnects partway through listing capabilities (observed on this hardware).
- Another program (VueScan, another scanning driver, a second copy of this tool) holds the
  scanner.
- Unrelated devices such as webcams or capture cards appear in the system's device list.
- More than one matching scanner is connected: all are reported, each with its own identifier.
- The scanner reports a setting with no current value, an empty list of choices, or an unknown
  unit.
- Capabilities differ depending on whether a film holder is inserted.
- The owner presses Ctrl-C while the tool is talking to the scanner.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The tool MUST detect whether a PrimeFilm XA is attached and usable, and report
  its model name and connection identifier when it is.
- **FR-002**: Detection MUST only consider the scanner driver that supports the PrimeFilm XA,
  so that unrelated drivers cannot claim the scanner first (the "Device busy" failure seen
  during initial probing) and unrelated devices are never reported.
- **FR-003**: The tool MUST list every setting the scanner exposes, each with its name, a
  human-readable title and description, current value, allowed values or range, unit, and
  whether it is active and changeable.
- **FR-004**: The capability listing MUST begin with a summary of the key film-scanning
  capabilities: supported resolutions, color modes, bit depths, maximum scan area, and whether
  infrared capture is available.
- **FR-005**: Output MUST be human-readable by default, and available in a structured,
  machine-readable format on request.
- **FR-006**: The tool MUST be strictly read-only: it MUST NOT start a scan, move film, change
  a setting, run calibration, or write anything to the scanner.
- **FR-007**: The tool MUST release the scanner on every exit path (success, error, Ctrl-C) so
  that other programs can use it immediately afterwards.
- **FR-008**: The tool MUST distinguish, with a specific message, a suggested next step and a
  distinct exit status, at least these outcomes: scanner found, not found, busy, permission
  denied, driver unavailable, and disconnected during the operation.
- **FR-009**: The tool MUST never prompt for input; all behavior is controlled by command-line
  arguments so it can be used in scripts.
- **FR-010**: The tool MUST offer a verbose mode that shows the detection and query steps it
  performs.
- **FR-011**: Detection, listing, formatting and error handling MUST be exercisable without the
  physical scanner, using a simulated scanner that reproduces the PrimeFilm XA's settings and
  each failure outcome.

### Key Entities

- **Scanner Device**: a detected scanner; identified by connection identifier, vendor, model
  name and device type.
- **Capability (Setting)**: one adjustable or read-only property the scanner exposes; has a
  name, title, description, value type, unit, current value, constraint (list of choices or
  numeric range), and active/changeable flags.
- **Capability Report**: the summary of key film-scanning capabilities plus the full list of
  capabilities for one scanner device.
- **Detection Outcome**: the result of a run: found (with devices), not found, busy, permission
  denied, driver unavailable, or disconnected; each maps to a message, next step and exit
  status.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With the scanner on, the owner learns whether it is ready within 5 seconds of
  running the detect command.
- **SC-002**: 100% of the settings the scanner exposes appear in the capability listing, with
  values matching an independent reference listing of the same scanner.
- **SC-003**: Each of the six detection outcomes produces a distinct message and exit status, and
  the owner can resolve each failure using only the suggested next step.
- **SC-004**: In 100% of runs, including failed and interrupted ones, another scanning program
  can open the scanner immediately after the tool exits.
- **SC-005**: The machine-readable output is accepted by a standard structured-data parser in
  100% of runs.
- **SC-006**: All non-hardware behavior is verified by automated checks that pass with no
  scanner attached.
- **SC-007**: After reading the code for this feature, the owner (a Rust beginner) can explain
  what each part does, with the help of its comments.

## Assumptions

- The tool is used by one person (the project owner) on a single Linux machine; no
  multi-user, network or remote-scanner support is needed.
- The scanner is reached through the open SANE "pieusb" driver, which lists the PrimeFilm XA's
  USB identity as a Reflecta RPS 10M / PrimeFilm XAs. If this driver turns out not to work with
  the XA, detection still reports a clear outcome and a different scanner backend is a separate
  feature (constitution Principle III).
- The SANE runtime library and development headers are installed (confirmed 2026-11-08).
- The scanner's USB device is already accessible to normal users (observed with open
  permissions); shipping a udev rule is out of scope unless testing shows it is needed.
- "Machine-readable" means JSON, the most widely parsed structured format.
- Previewing, scanning, film handling and image processing are out of scope for this feature.
- The command-line interface uses subcommands (for example `detect` and `capabilities`) so that
  later features (`preview`, `scan`) slot in alongside them.
