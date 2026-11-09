# Quickstart: Film Scanning

**Feature**: [spec.md](spec.md) | **Contract**: [contracts/cli.md](contracts/cli.md) |
**Experiments**: [research.md](research.md#hardware-experiments-run-before-the-code-that-depends-on-them)

## Prerequisites

- Feature 001 working: `primefilm detect` finds the scanner (colord fix applied, see README).
- At least 1 GB free in `/tmp` (pieusb buffers each frame there; 275 MB at `high`) and about
  6 GB free in the output folder per roll at the default preset.
- A test strip of 4–6 frames you don't mind handling repeatedly.

## Without the scanner

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Spot checks with the simulated scanner (`--simulate`, 6 simulated frames by default; use a
throwaway output folder):

| Command | Expect |
|---------|--------|
| `primefilm presets` | 4 presets, `standard` marked default, sizes as in data-model.md |
| `primefilm --simulate ready scan --roll t --output /tmp/pf` | exit 0; `/tmp/pf/t/t-001.tif` + `.json`; no frame advance logged |
| same again | `t-002.tif` (next free number) |
| `primefilm --simulate ready scan --roll t --frame 1 --output /tmp/pf` | `t-001-2.tif`; `t-001.tif` unchanged |
| `primefilm --simulate ready roll --roll r --output /tmp/pf` | exit 0, `end-of-film`, 6 frames `r-001`…`r-006` |
| `primefilm --simulate empty-gate roll --roll e --output /tmp/pf` | exit 0, 3 frames; the bare-light frame not saved |
| `primefilm --simulate jam roll --roll j --output /tmp/pf` | exit 9 after 2 frames; resume hint `--start-frame 3`; no visible partial file |
| `primefilm --simulate disconnect roll ...` | exit 7 after 2 frames |
| `primefilm --simulate no-film scan ...` | exit 8, no file |
| `primefilm --simulate ready preview --output /tmp/pf` | `/tmp/pf/previews/preview-*.tif`, 8-bit RGB |
| `primefilm --json --simulate ready roll ...` | one JSON document valid against `contracts/scan-output.schema.json` |

Open a saved `.tif` in the image viewer: the simulated gradient should display normally (not
transparent), and `tiffinfo` (from `libtiff-tools`) should show 4 samples, 16 bits,
`Extra Samples: 1<unspecified>`.

## With the scanner

1. **Run hardware experiments E1–E8** from research.md with `scanimage` before relying on
   film movement. Record results in `hardware-validation.md`. Stop if E2 shows the film
   moving on a non-advancing scan.
2. `primefilm preview` with a strip loaded: file appears in under 15 s (SC-002) and shows the
   frame in the gate; the film does not move.
3. `primefilm scan --roll test --preset web`: one file; run `primefilm preview` again and
   confirm the same frame is still in the gate (FR-014).
4. `primefilm advance`, then `primefilm preview`: shows the next frame (US5).
5. `primefilm roll --roll test --preset web` on the 4–6 frame strip: one file per frame, in
   order, nothing skipped or duplicated, ends with `end-of-film`, exit 0 (SC-001; repeat for
   the 10-run criterion over time).
6. Interrupt a roll with Ctrl-C during frame 2: frame 2 is saved, exit 130, resume command
   printed; reload the strip at frame 3, run the printed command, and confirm numbering
   continues (SC-007).
7. Time one frame at each preset (SC-003: standard ≤ half of the highest preset); record the
   per-preset times in research.md (E8).
8. After each step, `primefilm detect` succeeds immediately (scanner released, SC-006).
9. `cargo test -- --ignored` with the strip loaded.
