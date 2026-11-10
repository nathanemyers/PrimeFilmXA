# PrimeFilmXA Constitution

## Core Principles

### I. Archival Fidelity

The tool exists to digitize irreplaceable film. Every frame scanned gets exactly one archive
master, and that master is sacred.

- The archive master is one of two kinds:
  - a **raw scan**, saved losslessly at the scanner's full captured bit depth (e.g. 16 bits per
    channel TIFF), including the infrared channel when the scanner provides one; or
  - a **processed master**, for film types whose processing is specified (for example B&W
    negatives), made from the capture during scanning: full resolution, lossless, at least
    16 bits per channel. The raw capture is then not kept, and any temporary data MUST be
    removed on every exit path.
- A film type MAY use processed masters only when its feature spec says so explicitly. Film
  types without specified processing MUST keep raw scans.
- Archive masters MUST never be overwritten or modified in place. Anything made from a master
  (share copies, crops, edits, downsizing) MUST be a separate derivative file, clearly named so
  it is never mistaken for the master.
- Every master MUST record the settings used to capture it (resolution, bit depth, exposure,
  backend, timestamp); a processed master MUST also record its film stock, every processing
  setting actually used, and the tool version, so it can be traced and its processing
  reproduced from a rescan.
- A failed or interrupted scan MUST NOT leave a file that looks complete; partial output is
  written to a temporary name and renamed only on success.

Rationale: a lost or degraded master may mean re-handling fragile film, or losing it outright.
Raw scans of a whole roll cost several gigabytes and the owner wants only finished images for
film the tool knows how to process; a lossless 16-bit processed master keeps the tonal detail
that matters, at the accepted cost that changing its processing means rescanning the film.

### II. Hardware Safety

The PrimeFilm XA is a physical device moving physical film.

- The tool MUST NOT write firmware, calibration storage, or any persistent scanner setting that
  VueScan or the vendor software does not also write in normal use.
- The USB interface MUST be released on every exit path, including errors, Ctrl-C and crashes
  where catchable, so the scanner is never left claimed.
- USB disconnects and power loss mid-operation MUST be detected and reported clearly, never
  silently retried in a loop.
- Any operation that moves film (load, eject, advance) MUST be explicit and logged.

Rationale: the scanner has been observed dropping off USB; robustness here protects both the
hardware and the film inside it.

### III. Swappable Scanner Backend

All scanner I/O MUST go through a single backend interface (connect, list capabilities,
preview, scan, eject, release). Nothing outside that interface may talk to SANE, VueScan or
USB directly.

- The preferred backend is the open SANE `pieusb` driver (USB ID `05e3:0144`).
- A VueScan-driven backend or a direct-USB backend MAY be added behind the same interface if
  `pieusb` proves insufficient.
- Hardware discoveries (USB IDs, quirks, protocol captures, working option sets) MUST be
  recorded in the repository so they are not lost.

Rationale: SANE support for this exact model is unverified; isolating the backend keeps the rest
of the tool independent of which driver path wins.

### IV. Testable Without Hardware

- Image processing, file naming, metadata and workflow logic MUST be testable with no scanner
  attached, using a fake backend and recorded sample scans as fixtures.
- Tests that need the physical scanner MUST be clearly marked and skipped by default; they run
  only on explicit request.
- New behavior in hardware-independent code MUST ship with automated tests.

Rationale: the scanner is not always connected or powered, and tests must not move film.

### V. Simple, Scriptable First

- Every capability MUST be usable from a command-line interface with non-interactive flags, so
  batch jobs can be scripted. Errors go to stderr with a non-zero exit code.
- Any graphical or web interface is a layer over the same core, never a separate code path.
- Features MUST be justified by a real scanning need; speculative options, plugins and
  configuration layers are out of scope until a need appears (YAGNI).

Rationale: this is a personal tool for one scanner; simplicity keeps it maintainable.

### VI. Learning by Building

The project owner is learning Rust through this codebase, so the code is also a teaching text.

- Code MUST be idiomatic, readable stable Rust. Prefer the clear form over the clever one:
  no custom macros, heavy generics, trait-object gymnastics or explicit lifetimes unless the
  problem genuinely needs them, and then with a comment explaining why.
- The first time a Rust concept appears in the codebase (ownership and borrowing, `Result` and
  `?`, enums with `match`, traits, iterators, error types, threads or async, FFI), a short
  comment MUST explain what it does and why it is used there. Later uses need no repeat.
- Every plan MUST include a "Rust concepts introduced" list so each feature is also a lesson,
  and tasks SHOULD be ordered so concepts build on one another.
- `unwrap()` and `expect()` are allowed only in tests and in cases proven impossible to fail,
  with a comment saying why; real errors are handled with `Result`.
- Third-party crates MUST be well-established and each one justified in the plan; the code
  MUST NOT hide core logic behind a crate the owner cannot read.

Rationale: a tool the owner cannot understand cannot be maintained by them; teaching through
real code makes the learning stick.

## Platform & Hardware Constraints

- Target platform: Linux (currently Ubuntu, kernel 7.x). Other platforms are not supported
  unless a spec explicitly adds them.
- Target hardware: Pacific Image PrimeFilm XA, which enumerates as Genesys Logic `05e3:0144`
  and is listed in SANE as "Reflecta RPS 10M (aka Pacific Image PrimeFilm XAs)".
- The tool MUST run as a normal user with no root privileges at runtime. Any required udev rule
  MUST be shipped in the repo and documented.
- Implementation language: Rust, stable toolchain installed via `rustup`, built with Cargo.
  Dependencies come from crates.io or Ubuntu packages (e.g. `libsane-dev` for SANE bindings).
- `unsafe` code is permitted only inside the module that wraps C libraries (FFI), and every
  `unsafe` block MUST carry a `// SAFETY:` comment explaining why it is sound.
- Large scans MUST be streamed or processed in bounded memory where the backend allows it.

## Development Workflow

- Features follow the Spec Kit flow: `/speckit-specify` → (optional `/speckit-clarify`) →
  `/speckit-plan` → `/speckit-tasks` → `/speckit-implement`.
- Every plan MUST include a Constitution Check against the six principles above and justify
  any deviation in its Complexity Tracking section.
- Code MUST pass `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` before a
  task is marked done.
- Hardware-touching changes MUST be verified against the real scanner before being marked done,
  and the verification (date, film type, result) noted in the feature's spec folder.
- Commits are small and descriptive; generated scans and large fixtures are not committed
  unless deliberately chosen as test fixtures.

## Governance

- This constitution supersedes other project practices. Where a spec, plan or task conflicts
  with it, the constitution wins until it is amended.
- Amendments are made with `/speckit-constitution`, recorded with a Sync Impact Report, and
  committed separately from feature work.
- Versioning follows semantic versioning: MAJOR for removing or redefining a principle, MINOR
  for adding a principle or section or materially expanding guidance, PATCH for clarifications.
- Plans and reviews MUST check compliance with the Core Principles; added complexity MUST be
  justified in writing.

**Version**: 2.0.0 | **Ratified**: 2026-11-08 | **Last Amended**: 2026-11-09
