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

The unit tests cover the step wedge (SC-004), synthetic dust and star field
(SC-005) and determinism. *(Revised 2026-11-09: no tone balancing, JPEG or JSON files.)*

Spot checks with the simulated scanner (use a throwaway output folder):

| Command | Expect |
|---------|--------|
| `primefilm films` | `bw` (default), `hp5`, `fp4`; "Color film is not supported yet." |
| `primefilm --simulate ready scan --roll t --film hp5 --output /tmp/pf` | exit 0; `t/t-001.tif`, nothing else |
| `tiffinfo /tmp/pf/t/t-001.tif` | 1 sample per pixel, 16 bits, min-is-black; ImageDescription holds the JSON record (`film_stock` `hp5`, `inversion` `linear`, dust `off`) |
| same scan with `--dust normal` | 3 dust specks removed; images differ from `t-001*` only at the specks |
| `primefilm --simulate jam roll --roll j --output /tmp/pf` | exit 9; `ls -a /tmp/pf/j` shows no `.partial` and no raw file |
| `primefilm --simulate ready scan --film color` | exit 2, "Color film is not supported yet.", scanner not opened |
| `primefilm --simulate ready scan --brightness 10` | exit 2 (option removed) |
| `primefilm --simulate ready preview --output /tmp/pf` | `previews/preview-*.tif`, 8-bit gray positive |
| `primefilm --json --simulate ready roll ...` | valid against `contracts/scan-output.schema.json` (version 2) |

## With the scanner

1. **Run hardware experiments E9–E11** from research.md. Record results in
   `hardware-validation.md`. E11 keeps raw captures with the hidden `--keep-capture DIR`; keep
   them outside the repo.
2. Tune profiles and dust levels on the E11 captures without rescanning:

   ```bash
   cargo run --example process_capture -- CAPTURE.tif --film hp5 --dust normal --out /tmp/tune
   ```

3. `primefilm preview --film hp5` on an HP5 strip: a positive preview in under 15 s.
4. `primefilm scan --roll test --film hp5 --preset web`: open `test-001.tif`; it is a normal
   positive with no outlines or flat patches. No raw file in the roll folder.
5. `primefilm roll --roll hp5-test --film hp5` on a full strip at `standard`: compare the total
   time with 002's 91 s per frame (SC-002: at most 20% longer) and the folder size with the raw
   size (SC-003: at most one third).
6. Repeat 5 with FP4.
7. Judge the results (SC-001): no outlines or flat patches, and how many frames need more
   than ordinary finishing in post.
8. Rescan a dusty frame with `--dust gentle|normal|strong` and compare with `--dust off`
   (SC-006): any outlines or flat patches mean the dust constants need tuning (E11).
9. After every run, `ls -a` the roll folder: only `.tif` files (SC-007).
10. `cargo test -- --ignored` with an HP5 strip loaded.
