# ICC Color Lookup integration

Parent: please add `mod icc;` under `mod lookup;` in crates/compositor/src/adjust.rs. API is `Adjustment::color_lookup_from_icc(bytes: &[u8], size: u32) -> EngineResult<Self>`.

Implemented engine sampling in color-mgmt; six generated-profile unit tests and full color-mgmt suite pass. Abstract looks use encoded sRGB -> abstract PCS -> sRGB; RGB-to-RGB device links evaluate directly. Other classes, non-RGB links, malformed input and cube sizes outside 2..=256 reject explicitly. ICC profiles generated using actual installed lcms2 API, serialized, then loaded back.

Compositor adapter and four integration tests are now ready. The compositor targeted test run reached the expected RED state: no `color_lookup_from_icc` method (module not wired yet). PSD compilation is no longer blocking. Please wire `mod icc;` and rerun:

```
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26 cargo test -p compositor --test m5_26_icc
```

The integration tests cover generated abstract brightness, a real LCMS-generated RGB device link, rejected malformed/display/CMYK inputs and grid bounds, and rendering through ColorLookup while preserving alpha. Compositor has added `color-mgmt` dependency and `lcms2` dev-dependency only. Optional PhotoFilter presets omitted to focus on ICC correctness.

TDD evidence: missing sampling API RED -> abstract test GREEN; RGB link transform creation RED -> direct-link GREEN; wrong class/grid/CMYK rejection RED -> validation GREEN. Full `cargo test -p color-mgmt` passes, including all existing integration tests. No global formatting or commit performed; only newly owned Rust files formatted. Existing compiler warnings are outside owned files.
