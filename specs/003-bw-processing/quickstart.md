# Quickstart: Black-and-White Negative Processing

**Feature**: [spec.md](spec.md) | **Contract**: [contracts/cli.md](contracts/cli.md) |
**Model**: [data-model.md](data-model.md) | **Experiments**:
[research.md](research.md#hardware-experiments-run-before-the-code-that-depends-on-them)

## Prerequisites

- Feature 002 working (`primefilm roll` scans a strip end to end).
- About 2 GB free per roll at the default preset (36 MB master + ~4 MB JPEG per frame).
- B&W strips of HP5 and FP4 with 4–6 frames, ideally including a dusty frame, an unexposed
  frame (leader) and a night or high-key frame.

## Without the scanner

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

The unit tests cover the step wedge (SC-004), synthetic dust and star field (SC-005),
determinism, blank frames and tone matching.

Spot checks with the simulated scanner (use a throwaway output folder):

| Command | Expect |
|---------|--------|
| `primefilm films` | `bw` (default), `hp5`, `fp4`; "Color film is not supported yet." |
| `primefilm --simulate ready scan --roll t --film hp5 --output /tmp/pf` | exit 0; `t/t-001.tif`, `t-001.jpg`, `t-001.json` and nothing else |
| `tiffinfo /tmp/pf/t/t-001.tif` | 1 sample per pixel, 16 bits, min-is-black |
| `jq .processing /tmp/pf/t/t-001.json` | `film_stock` `hp5`, points with `source` `auto`, dust `normal` with a repaired count > 0 |
| same scan with `--dust off` | `dust.repaired` 0; image differs from `t-001.tif` only at the specks |
| same scan with `--brightness 30` | midtones lighter than `t-001`, black and white unchanged |
| `primefilm --simulate ready roll --roll m --match-tones --output /tmp/pf` | every `m-*.json` has the same `black_density` / `white_density`, `source` `matched` |
| `primefilm --simulate interrupt roll --roll i --match-tones --output /tmp/pf` | exit 130; resume command includes `--black` and `--white` |
| `primefilm --simulate jam roll --roll j --output /tmp/pf` | exit 9; `ls -a /tmp/pf/j` shows no `.partial` and no raw file |
| `primefilm --simulate ready scan --film color` | exit 2, "Color film is not supported yet.", scanner not opened |
| `primefilm --simulate ready scan --black 0.2` | exit 2 (needs `--white`) |
| `primefilm --simulate ready roll --brightness 80` | exit 2 (range −50…50) |
| `primefilm --simulate ready preview --output /tmp/pf` | `previews/preview-*.tif`, 8-bit gray positive |
| `primefilm --json --simulate ready roll ...` | valid against `contracts/scan-output.schema.json` (version 2) |

Open `t-001.jpg` in the image viewer and a browser: it should display as a gray image.

## With the scanner

1. **Run hardware experiments E9–E11** from research.md. Record results in
   `hardware-validation.md`. E11 keeps raw captures with the hidden `--keep-capture DIR`; keep
   them outside the repo.
2. Tune profiles and dust levels on the E11 captures without rescanning:

   ```bash
   cargo run --example process_capture -- CAPTURE.tif --film hp5 --dust normal --out /tmp/tune
   ```

3. `primefilm preview --film hp5` on an HP5 strip: a positive preview in under 15 s.
4. `primefilm scan --roll test --film hp5 --preset web`: the `.jpg` looks like a normal print;
   no raw file in the roll folder.
5. `primefilm roll --roll hp5-test --film hp5` on a full strip at `standard`: compare the total
   time with 002's 91 s per frame (SC-002: at most 20% longer) and the folder size with the raw
   size (SC-003: at most one third).
6. Repeat 5 with FP4.
7. Judge the results (SC-001: 9 of 10 frames look right without adjustment; SC-006: 9 of 10
   need no further dust spotting). Record the counts.
8. A frame that looks wrong: rescan with `--brightness`/`--contrast`, and note which way the
   defaults should move.
9. `roll --match-tones` on a strip shot in one light: frames look equally bright.
10. Interrupt a matched roll, reload at the next frame, run the printed resume command: later
    frames record the same points.
11. After every run, `ls -a` the roll folder: only `.tif`, `.jpg`, `.json` (SC-007).
12. `cargo test -- --ignored` with an HP5 strip loaded.
