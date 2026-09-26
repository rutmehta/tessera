# M5-26 live Shadows/Highlights CPU fallback

Implemented `Compositor::render_tile_with_neighbourhood(&self, &Document, TileCoord) -> EngineResult<Tile>` in `crates/compositor/src/render/exec.rs`. Returns straight planar f32 RGBA, matching `render_tile` output format.

- Every call uses a fresh zero-cache compositor and full tile regions. It does not bake document state, populate/reuse the caller's root/group cache, or rely on tile-local damage stamps.
- Positive-radius adjustments gather actual neighbouring backdrop tiles by compiling/replaying the program up to the current layer ID. Prefix return uses the current TOP frame, not root. Earlier neighbourhood adjustments recursively replay strictly earlier prefixes.
- Halo samples are unpremultiplied into padded straight RGBA; only document boundaries are replicated. Existing `apply_padded` computes the local operator; existing `adjust_px` applies mask, mode, opacity/fill, preserving alpha.
- Ordinary isolated groups compile inline Push/Pop with this zero-cache compositor, so their children are supported. Pass-through and clipping frames also work.

## API restrictions

The standard cached `render_tile`/`render_level`/pyramid path still rejects positive-radius neighbourhood execution. Callers must explicitly opt into `render_tile_with_neighbourhood`. Resident GPU is not implemented by this fallback. The standard executor error now directs callers to the live API.

Layer-styled documents return `EngineError::Unsupported` before compilation: eager style source compilation cannot safely replay prefixes. Positive-radius adjustments inside smart-object child documents remain unsupported because child rendering uses the standard executor. Ordinary smart sources without such child adjustments are unaffected.

This is an intentionally expensive reference path: zero persistent memoization and recursive prefix replay can multiply work for long neighbourhood-adjustment stacks. No performance guarantee or Adobe numerical-equivalence claim.

## Verification

Observed initial test compile failure because the requested API did not exist, then implemented and verified it.

Command (release):

```sh
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26 cargo test -p compositor --release --test m5_26_shadows --test structure --test caching --test contracts
```

Results: shadows 14 passed; structure 11 passed; caching 5 passed; contracts 3 passed. Existing LibRaw warnings and unrelated Rust documentation/parentheses warnings remain.

New live tests cover L0/L2 tile seams against whole-backdrop reference, sequential positive-radius adjustments, root/isolated/pass-through/clipping frames, repeated adjacent-tile paint edits using the same public compositor, caller-cache nonpopulation, mask/Multiply/opacity/partial-alpha behavior, and explicit styled-source rejection. Existing standalone operator tests and standard-path rejection remain.

Changed only `render/exec.rs`, `tests/m5_26_shadows.rs`, and this requested notes file. No `render/mod.rs` or `adjust/shadows.rs` edits, no commits, no global formatting.
