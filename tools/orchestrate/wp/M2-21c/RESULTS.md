# M2-21c export throughput — results

Apple silicon (this machine), shared with another work package's build (load 3–10).
Single runs. Default recipe (Auto lens profile + CA). `gpu_bench`, isolated
processes, decode + render + encode, seconds. "Before" is the M2-21b code rerun
in this session at similar load.

## Five fixtures

| Fixture | Web before → after | Full before → after |
|---|---:|---:|
| Canon CR3 (lens map) | 3.72 → 0.98 | 3.58 → 0.80 |
| Sony ARW | 0.99 → 0.37 | 2.11 → 0.47 |
| Nikon NEF (36 MP) | 2.55 → 0.81 | 4.42 → 1.17 |
| Fuji RAF (X-Trans, map) | 1.80 → 0.42 | 3.75 → 0.65 |
| DNG | 1.36 → 0.36 | 2.79 → 0.51 |

Web uses the benchmark's render_scale 2 (as the FFI's export_scale does).
Nikon full breakdown: decode ~0.4, lens analysis 0.17, GPU bands 0.30–0.34
(was 3.4), JPEG encode + commit 0.07 (was 1.6–2.2).

**100 Web JPEGs** (`hundred_web_exports`, 20 × each fixture): 176.3 s → 26.0 s
render + encode; with decode serialised on top (0.13 s/image) 39.0 s.
Developing Web exports at full resolution (gate-passing, below): 37.8 s
(50.8 s with serial decode).

docs/08 targets: 45 MP < 1.5 s — 36 MP now 1.0–1.2 s including decode
(extrapolates to ~1.3–1.5 s at 45 MP; no 45 MP fixture). 100 JPEGs < 40 s —
met for render + encode (26 s); 39 s only if decode is serial and added.

## What changed

1. **Whole-band sensor dispatches.** `Renderer::render_export_rows` (image-core)
   renders full-width row bands with one dispatch per stage: rows uploaded once
   from the contiguous CFA plane (`CfaPyramid::pixels`), each stage's input
   gathered once with its halo by one fold-at-edges kernel (`band.wgsl`, same
   rule as `resample::clamp_phase`), then highlights, demosaic, lateral CA,
   resample, matrices, vignette, Detail, fused point chain, map, display. 14–17
   dispatches per band (was 9–20k). Halo is a gather dispatch, not in-kernel:
   the verified operators are unchanged. Origins are explicit (`*_at` trait
   methods) because bands start at arbitrary rows.
2. **Two bands in flight.** Two band workers, each with half the 384 MiB budget
   (reserved for the worker's life, covering its recycled buffers); one encodes
   and uploads while the other executes; each waits only on its own submission
   index. Bands are sized greedily from a per-pixel scratch model (level and map
   aware, map row spread precomputed per 16-row block in parallel). Workers
   reuse their own buffers band to band.
3. **Parallel JPEG.** Stripes of whole MCU rows encoded on rayon and stitched
   with restart markers. Byte-identical to one sequential encode with that
   restart interval and decodes to the same pixels as the unstriped encode
   (test `striped_jpeg_matches_sequential_restart_encode`, 4:2:0 and 4:4:4,
   ragged edges, ICC + XMP + density). No new dependency.
4. **Batch.** The export pipeline runs two renders at once for outputs ≤ 16 MP
   (overlaps one image's lens analysis with the other's GPU), encoder unchanged.
5. **Resize.** Band resize reads exactly its Lanczos support (no 256-row
   alignment); the Lanczos pipeline is compiled once per device.

## Precision

- Scale 1 (`five_fixture_full_chain_tolerance`, now asserting the band path):
  identical to M2-21b — max linear 1.8e-6…9.7e-4, 1 code, all five.
- Band renderer vs tile renderer (`band_renderer_matches_tile_renderer`,
  normal suite): Bayer and X-Trans, crop offset, colour highlights,
  vignette + grain, lens map, levels 0/1, resized or not, whole or 16-row bands:
  < 1e-5.
- **Web-scale gate** (`five_fixture_web_scale_tolerance`, vs CPU full-res render
  resized by the CPU exporter, LongEdge 2048):
  - full-resolution development + GPU resize: ≤ 3.2e-4 linear, 1 code, all five — passes.
  - pyramid-level development: Sony 26 codes (1.3% of samples > 1 code), Nikon 45
    (4.5%), Fuji 56 (16%), DNG 41 (8%); Canon identical to full-res. **Fails**
    §1.3 by construction (tone/Detail/output encoding do not commute with the box
    downsample).
  - So the auto pyramid-level Web path exists but is **off by default**
    (`TESSERA_EXPORT_WEB_LEVEL=1`). Note the FFI already passes render_scale > 1
    for resized exports (`export_scale`), which renders at level L as in M2-21b —
    that product path remains ungated and fails this gate; switching the FFI to
    render_scale 1 would make it gated at ~1.45× the Web batch time (37.8 s).

## Viewport

`export_batch_does_not_starve_slider_drag` passes: slider during export p50 4.5
/ p90 5.9 / max 15.6 ms (idle 4.2 / 5.3 / 6.4), 120/120 frames at L2, export of
5 full-size images 5.6 s (was 13–15 s). Yield before every band kept; the band
path has no mid-band checkpoints (a band is ≤ 4 MP, ~10–20 ms GPU).

## Verification

- `cargo test -p export -p image-core -p pipeline-gpu -p jobs -p tessera-ffi --release`: exit 0, 72 test binaries ok.
- `cargo clippy --release -p export -p image-core -p pipeline-gpu -p jobs -p tessera-ffi -p raw-decode --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all --check`, `git diff --check`: clean. engine-api and apps/mac unchanged.
- Opt-in, run: `five_fixture_full_chain_tolerance`, `five_fixture_web_scale_tolerance`,
  `five_fixture_export_benchmark`, `hundred_web_exports`.
- Partial whole-workspace run (stopped on request): agent, engine-api, ml-quality,
  cull, index, library, previews, pipeline-cpu, color-mgmt, lens, raw-decode,
  libraw-ffi, sidecar, style-profile, jobs, tessera-mcp all passed.

## Unfinished / limits

- Full `cargo test --workspace --release` not completed (see above).
- Web pyramid-level path fails the Web-scale gate; off by default; FFI render_scale path unchanged and ungated.
- Halo handled by a gather dispatch per stage, not inside the operator kernels.
- Texture/Clarity/Dehaze still use the tile renderer (whole-level barrier).
- Band scratch is an estimate (150 + 40·(4^L−1) B/px, +48 B/px mapped); a band over
  budget makes the whole export fall back to the tile renderer. Recycled buffers are
  not counted in a transaction's allocation check (bounded by the worker's reservation).
- Detail filter is now the most expensive kernel (~2.2 ms per 0.56 MP band); lens
  analysis (0.17–0.25 s) and decode dominate per-image time.
- No 45 MP fixture; the 1.5 s target is extrapolated.
