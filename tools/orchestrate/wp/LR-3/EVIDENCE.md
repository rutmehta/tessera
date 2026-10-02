# LR-3 verification evidence

Implementation tested: `31562ff18e38100489f5f79f3fd96c393b665004`.
Source and test code did not change during the gate or continuation.

## Initial RED

Commit `68a042a976daed9b350a8835a27dc363b73759f9`:

```text
cargo test --locked -p import-lrcat --test retouch
4 mapping failures, 1 retention pass
Expected 1 retouch operation; imported 0.

cargo test --locked -p tessera-ffi --test lr3_retouch_import -- --nocapture
lr3_synthetic_catalog_clone_pixels: FAILED
assertion left == right: left 0, right 1
```

Additional tests committed at `1b2b067f9653c9abb3f9e7e42638a814fcb4fadd` caught
unknown-method consumption and unresolved inherited XMP prefixes in the initial
translator draft. Both passed after the implementation fix.

## GREEN feature checks

```text
running 7 tests
test lr3_brush_dabs_and_source_anchor ... ok
test lr3_legacy_string_heal_and_empty_alias ... ok
test lr3_unknown_method_is_not_silently_dropped ... ok
test lr3_malformed_or_unrepresentable_key_is_retained_atomically ... ok
test lr3_xmp_attributes_and_legacy_items ... ok
test lr3_spot_coordinates_offsets_and_units ... ok
test lr3_xmp_inherited_namespace_prefix_is_not_semantic ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

LR-3 synthetic 128x80: centroid_error_px=0.00000000, center=0.50000000, feather_shoulder=0.17323937
test lr3_synthetic_catalog_clone_pixels ... ok
```

The final pixel test strengthens the shoulder check to `0.17323937 ± 1e-5`;
it also passed in the continuation. The initial check was a broader interval.
Center tolerance is 1e-5 and centroid position tolerance is 1 px.

## Full gate and continuation

```text
cargo test --locked -p import-lrcat -p engine-api -p tessera-ffi -- --test-threads=3
exit 101: 523 passed, 1 failed, 15 ignored

32 remaining tessera-ffi test binaries, --no-fail-fast, --test-threads=3
exit 0: 204 passed, 0 failed, 16 ignored

cargo test --locked --doc -p import-lrcat -p engine-api -p tessera-ffi
exit 0: zero doc tests

cargo clippy --locked -p import-lrcat -p engine-api -p tessera-ffi --all-targets -- -D warnings
exit 0

cargo fmt --all --check
exit 0

git diff --check
exit 0
```

The failed test is `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`.
Neither its code nor its 250 ms p95 threshold was changed. Measurements:

```text
test-gate.log: LIQUIFY 20MP (5472x3648, cell 16, proxy 1824x1216 /3): open 3556 ms; brush+preview per event median 289.8 ms, p95 583.7 ms, max 1395.4 ms (brush 7.7 ms, preview 278.8 ms median); full-res apply 3646 ms
liquify-retry.log: test brush_latency_on_a_20_megapixel_layer ... LIQUIFY 20MP (5472x3648, cell 16, proxy 1824x1216 /3): open 2787 ms; brush+preview per event median 212.4 ms, p95 266.1 ms, max 310.4 ms (brush 7.7 ms, preview 205.1 ms median); full-res apply 2460 ms
```

The isolated retry used `--exact --nocapture --test-threads=1` and also exited
101. These results do not establish that LR-3 caused the failure or that machine
contention is its only cause. No threshold was relaxed.

## 29c source-output comparison

Same 20-image synthetic fixture shape at starting HEAD
`87ff1ff173e4d6d2053a534b7cfd9343aef906e1` and the new implementation. Only recipe
image IDs were normalized. JSON byte slices were compared per imported image.

```text
CHANGED_IDS [1003, 1013]
BYTE_IDENTICAL_ROWS 18
CHANGED_PATHS ['/recipe/crs:RetouchInfo', '/recipe/history/entries', '/recipe/history/head', '/recipe/lrcat_develop_source/properties/RetouchInfo', '/recipe/settings/locals/retouch']
```

The full 2,000-image import golden and streaming/PlanJson parity tests pass with
the intentional retouch digest update listed in HANDOFF.md. All newly added
fixtures are synthetic. No Swift gates or application launch were performed.
