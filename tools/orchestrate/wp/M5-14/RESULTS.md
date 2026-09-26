# M5-14 implementation and verification

## Implemented

- Typed, serialized layer styles and document global light; undoable light/style edits.
- CPU alpha-derived shadows/glows, bevel/emboss, satin, solid/gradient/pattern overlays and inside/center/outside strokes, repeated effects, scaling, effect blend modes/opacity, independent fill opacity, and whole-layer opacity.
- Interior shape coverage applied once to preserve antialiased alpha. Full-source evaluation crosses tile boundaries. Conservative full damage and revision invalidation for styled trees.
- Ordered smart filters on the nested composite before resampling, per-filter blend options, one shared child-space filter mask, immutable sources, bounded cache keyed by child namespace/source revision/parameter hash, evaluation counters, undoable filter edits, native persistence.
- Dependency-inverted `SmartFilterEvaluator` and `filters::CompositorFilters` adapter using existing Filter/halo implementations. Optional `filters/camera-raw-filter` calls pipeline-cpu tone processing on RGB tiles.
- Native PSD lfx2 basics and global-light resources. Unknown/unsupported records retained, including opaque SoLE records. Tests serialize actual PSD bytes, parse them again, and render visible styled layers.

## Verification actually run

Required command:

    cargo test -p compositor -p filters --release && cargo clippy -p compositor -p filters --all-targets -- -D warnings && cargo fmt --check

Exit 0. Test totals parsed from `verification.log`: 145 passed, 0 failed, 6 ignored (existing benchmark tests).

Additional:

    cargo test -p filters --release --features camera-raw-filter
    cargo clippy -p filters --all-targets --features camera-raw-filter -- -D warnings

Both finished successfully. Feature-suite totals: 54 passed, 0 failed, 2 ignored. See `camera-raw-verification.log`.

`git diff --check` passed. All changed/new paths were programmatically checked against the WP allowlist. Engine-api, resident/**, gpu.rs, blend.rs, and existing golden files were not changed. No commit or push performed. CARGO_TARGET_DIR remained `/Volumes/betterSSD/tessera-cache/target/M5-14`.

LibRaw's existing C/C++ build warnings appear in logs; Rust clippy with `-D warnings` succeeds.

## Integration constraints and approximation boundaries

- Before selecting the resident renderer, the host must call `DocState::check_resident_effects()` and fall back to CPU on `Unsupported`. This WP deliberately does not edit the concurrent M5-08b resident files, so the resident implementation itself does not yet call that preflight. The per-tile GPU port rejects styled source operations.
- Install `filters::CompositorFilters` for the complete existing filter inventory. Standalone compositor supports invert/Gaussian and explicitly rejects other enabled filters without an evaluator.
- Style morphology is square-footprint; bevel uses a blurred alpha height field. Contour/jitter are stored placeholders, bevel texture is not implemented, and overlay fill coordinates do not scale with kernel geometry. This is not an Adobe pixel-equivalence claim.
- Style evaluation is correctness-first and recomputes full-source planes for each uncached output tile. It is bounded but not an interactive performance optimization.
- Styled adjustment/pass-through layers and filter-mask feather return explicit `Unsupported` errors.
- PSD exports support native lfx2 basics, not every runtime effect. Unsupported newly authored PSD styles fail explicitly. SoLE/filter-effect records remain opaque on the existing rendered-proxy import path: generic descriptors are parsed, but an executable filter schema plus unfiltered embedded source is not available. Filters must not be double-applied to rendered proxies.

Full behavior, ordering, knockout semantics, bounds, and limitations are documented in `crates/compositor/COMPOSITOR.md` §9.1.
