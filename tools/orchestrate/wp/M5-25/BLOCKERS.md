# M5-25 Round 2 handoff: gate passes, acceptance remains incomplete

All edits remain inside the permitted worktree and allow-list. No commits were made. No edits to compositor document/edit/format or filters remove/distraction. `kanban_show()` has no task binding in this execution, so this file is the durable handoff.

## Implemented and retained

- Default `camera-raw-filter`, full `DevelopSettings` JSON plus amount, shared engine validation and explicit AI-mask error.
- Straight document-linear RGBA boundary, FilterContext supplied by CPU/resident renderers, profile-byte-aware filter-output cache identity.
- CPU reference RGB Develop chain, color-mgmt matrix-shaper ICC conversion to/from linear Rec.2020, unchanged alpha and amount blending. Actual decoded-PNG image-core goldens in sRGB and Display P3, serialization/undo tests.
- Documented shared-device resident-buffer import/export bridge.

## Added in this execution

- Replaced resident Dehaze host readbacks with exact GPU order statistics: dark/luminance quantiles, candidate RGB medians, airlight and confidence. No intermediate pixel/statistics readback or submission. Statistics are transaction-local, not cached across potentially abandoned batches.
- GPU procedural local groups: linear/radial/brush/luminance/color masks, combine/invert, immutable pre-local adjustment deltas. WB/tone/presence/color/hue/sharpness/signed noise use GPU operators. Point/detail operators are row-tiled with real halos, allowing 24MP input without float-index loss. Matches existing CPU limits (local moire remains its validated no-op; local defringe/color-overlay error).
- Public `resident_rgb_optics::RgbOpticsPlan` wraps existing resident CA/gain/remap kernels. Tests cover genuine resolved calibration supplied by caller. Camera Raw integrates manual vignetting, distortion, crop/straighten/manual transforms, including crop-aware effects and canvas padding.
- Integrated Dehaze/locals/manual optics into Camera Raw rather than merely exposing unused helpers.
- Regression tests cover missing capability RED followed by GREEN, alpha/amount, tile edges, signed/HDR P3, crop/straighten padding, dehaze cold/repeated/abandoned transactions and exact-statistics edge cases, procedural masks and individual local operators.

## Remaining implementation gaps

This is NOT the full resident Develop engine requested by M5-25:

1. Automatic lens/image-derived CA analysis remains CPU-only. The default `lens.profile=Auto` with automatic CA is explicitly declined, not silently treated as identity. The optics helper can consume a genuinely resolved calibration but Camera Raw has no resident resolver yet. Supported resident recipes still require `lens.profile=None` and `remove_chromatic_aberration=false`.
2. Defringe, Upright/orientation/constrain-crop and lens blur remain declined by the resident path. Depth-dependent effects require a depth provider absent from this adapter. Manual optics retain the existing less-than-2^24-pixel whole-frame operator limit, so the 24MP benchmark does not enable optics.
3. CPU uses pipeline-cpu's in-memory RGB reference entry point, not an image-core Renderer instance with internal stage memoisation. image-core source constructors remain outside the allow-list. Compositor per-filter output/prefix caching is active, but internal Develop-stage memoisation is not added.
4. Consequently the required fully general full-chain GPU benchmark/coverage remains incomplete. Existing supported-subset performance is not represented as full acceptance.

Do not remove capability guards or invoke CPU pixel operators under a resident label to close these gaps.

## Verification actually run by the parent

Kept `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-25` throughout.

    cargo test -p filters -p compositor -p pipeline-gpu --release && cargo clippy -p filters -p compositor -p pipeline-gpu --all-targets -- -D warnings && cargo fmt --check

Final exit status: 0. Output: `gate.log`. Aggregate results parsed from this log: 388 passed, 0 failed, 26 ignored. Existing LibRaw C++ warnings remain; Rust clippy with -D warnings passed. `git diff --check` passed.

Camera Raw GPU test binary: 5 passed, 1 benchmark ignored in the normal run. CPU golden binary: 6 passed. Pipeline procedural locals: 7 passed, one 24MP regression ignored in normal gate. Optics bridge: 4 passed. Dehaze resident local-tone suite: 8 passed, one benchmark ignored.

Explicit parent benchmark:

    cargo test -p filters --release --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --nocapture

Exit status: 0. 6000x4000 supported recipe (WB/detail/basic tone/Texture/Clarity/Dehaze/curve/HSL/procedural exposure+saturation/vignette/grain): CPU=33.405863333s, GPU=3.202695s. Cold GPU pipelines included, upload/readback excluded. Sampled parity <=2e-3 and exact alpha assertions passed. Output: `bench.log`. Does not enable automatic lens stages or manual optics.

The delegated local implementation also exercised its ignored 24MP regression with presence, hue and signed detail controls; parent independently verified the integrated 24MP Camera Raw benchmark above.

RESULT: FAIL full resident lens/geometry coverage and internal Develop-stage memoisation remain incomplete despite the passing required gate.
