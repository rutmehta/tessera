# M3-21 — PASS under the coordinator's round-2 scope

## Scope delivered

- Apache-2.0 `ml-filters` workspace member with fallible, cancellation-aware raster
  filters and offline catalog metadata. Catalog contains exactly **Skin Smoothing,
  Colorize, JPEG Artifact Removal**.
- Deterministic face-box/color-masked skin frequency separation with Blur and
  Smoothness, texture residual preservation, and synthetic regression tests.
- Apache-2.0 DDColor paper-tiny ONNX, immutable URL and SHA-256 pinned in the model
  registry, Lab reconstruction, chroma artifact reduction, saturation, and hints.
- JPEG-quality-conditioned blend through the existing tiled/padded DRUNet path.
- `Photo Restoration (no face model)` remains a denoise-only API outside the
  shipped catalog. Nonzero Enhance face and Scratch reduction return errors.
- GFPGAN is excluded by the round-2 decision because of StyleGAN2/NVIDIA
  non-commercial license lineage. No GFPGAN registry entry, runtime download,
  inference, or permissive-license claim was added. README, model provenance,
  registry comments and `docs/13-licensing.md` record the decision.

## CPU execution policy

Added `ExecutionPreference::{CpuOnly, PreferCoreMl}` and
`SessionOptions::with_execution_preference(...)`. This maps to the existing
`coreml` flag instead of introducing a conflicting second provider selector or
breaking existing complete struct literals. `CpuOnly` registers only ORT CPU.
`Colorize::load` explicitly opts into CpuOnly even when given CoreML options.

Default runtime behavior, fallback reporting, `PartitionReport::require_coreml`,
and the registry-wide strict CoreML test remain unchanged. Requesting a strict
CoreML audit of DDColor still fails; the shipped adapter intentionally uses CPU
instead. Other model adapters retain their caller-selected execution policy.

## Verification performed in round 2

Kept `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-21` for every
Cargo command. Ran the exact required chain:

```sh
cargo test -p ml-filters -p ml-runtime --release && cargo clippy -p ml-filters -p ml-runtime --all-targets -- -D warnings && cargo fmt --check
```

**Exit 0**, `round2-gate.log`. Parsed harness totals: **35 passed, 0 failed,
5 ignored**. Real-model tests return early in this ordinary gate when no cache
is configured, so this count alone does not establish real inference. Existing
LibRaw C++ deprecated-sprintf warnings persist; Rust clippy with `-D warnings`
and workspace formatting passed.

Also ran:

```sh
TESSERA_FILTER_MODEL_CACHE="$PWD/crates/ml-filters/training/.cache" cargo test -p ml-filters --release -- --nocapture
TESSERA_FILTER_MODEL_CACHE="$PWD/crates/ml-filters/training/.cache" cargo test -p ml-filters --release --test bench colorize_12mp_cpu -- --ignored --nocapture
```

Both **exit 0**, in `round2-real-models.log` and
`round2-bench-colorize-cpu.log`. The real cache is SHA-verified before use and
these test paths never download missing weights. DDColor executed with only CPU
provider assignments despite caller CoreML options. It validated dimensions,
finite bounded output, chroma, alpha, invalid controls and cancellation.
Real DRUNet q=30 JPEG regression: **41.317 -> 41.871 dB PSNR**.

The 4000x3000 DDColor benchmark measured **3.264 seconds apply**, excluding model
load and fixture creation, including adapter conversion and publication. It
reported **1854 executed CPU nodes**, no CoreML nodes, and confirmed the strict
CoreML guard rejects that report. One procedural-image timing, not a performance
distribution or photographic quality claim. Neural inference itself uses a
512x512 image with full-resolution chroma reconstruction.

Regression evidence:
- `round2-red-catalog.log`: old four-entry catalog failed the new three-entry test.
- `round2-green-catalog.log`: revised catalog and restoration naming passed.
- `round2-red-preference.log`: new API test failed to compile before the API existed.
- `round2-green-preference.log`: real convolution fixture ran on CPU with no
  fallback reason, and remained rejected by the CoreML guard.
- `round2-red-colorize.log`: real prior CoreML-requested DDColor violated the
  CPU-only assertion. The same test passed after the adapter opted into CpuOnly.
- Registry regression ensures no GFPGAN ID is present and cache-only DDColor
  lookup does not populate an empty cache.

`git diff --check` passed. All changed/untracked, nonignored files match the
allowed paths. No unignored weights, Python environments or target files were
added. No commits, pushes, compositor registration, or out-of-scope edits.
No Kanban task ID was injected, so there was no board card to transition.

## Prior-attempt benchmark evidence (not rerun in round 2)

The unchanged DRUNet/skin benchmark paths have prior retained measurements:

| Filter | Apply seconds | Provider audit | Log |
|---|---:|---|---|
| Skin Smoothing | 5.543 | CPU algorithm, no model | bench-skin.log |
| JPEG Artifact Removal | 227.948 | strict CoreML guard passed | bench-jpeg.log |
| Photo Restoration, denoise only | 235.745 | strict CoreML guard passed | bench-restoration.log |

Those DRUNet runs also emitted an E5RT teardown message about a convolution output
size being too small; preserved in the logs, not suppressed. Provider profiling
does not establish ANE versus CPU/GPU internal to CoreML. The historical
`bench-colorize.log` measured 5.897 seconds on mixed providers and failed the
strict guard. It is superseded for shipping by the round-2 CPU-only measurement,
not retroactively relabeled as a passing CoreML run. Other logs without a
`round2-` prefix belong to earlier attempts.

## Precisely not done

1. GFPGAN face restoration, face alignment/crop-to-512, restoration and feathered
   paste-back, or its real-model integration: **excluded**, not pending delivery.
   The guarded historical exporter remains unexecuted beyond its license guard
   and is not part of the shipping path. CodeFormer/GPEN were not substituted.
2. Scratch reduction: no approved model selected, explicitly unavailable.
3. DDColor CoreML acceleration/full CoreML residency: intentionally replaced with
   CPU-only execution under the scope decision; the guard was not weakened.
4. Compositor/UI adapter registration: deliberately left to the owning package.
5. Automatic face detection inside Skin Smoothing: callers provide face boxes
   (including through `Params::with_faces` for YuNet results); no implicit weights.
6. Interrupting an active ORT/DRUNet tiled call: cancellation is cooperative around
   inference and discards cancelled output, not in-flight preemption.

The trait returns `Result<Raster>` instead of bare `Raster` so missing weights,
invalid controls, model errors and cancellation cannot become successful edits.
This documented API deviation is retained for the later compositor adapter.

RESULT: PASS
