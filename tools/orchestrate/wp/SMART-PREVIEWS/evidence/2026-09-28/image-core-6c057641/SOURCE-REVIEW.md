# Final image-core Smart Preview source review

Reviewed immutable commit `6c05764180192b05a9fbaa553642c277ee1d6cc8`, the earlier `/tmp/tessera-smart-preview-image-core.patch` and `/tmp/tessera-smart-preview-route-review.md`. No source edits, builds or test execution. Compiler lane remained available to its assigned worker.

## Result

**No actionable correctness finding in this scoped delta. Approved at source level for the bounded camera-linear image-core route.** This is not an integration/UI release approval and does not establish that any tests passed.

## Delta and retained behavior

The production implementation matches the earlier reviewed patch: `source.rs`, `smart_preview_render.rs`, `resident_render.rs` and `rgb_render.rs` have identical changed lines. The only textual changed-line alignment difference in `render.rs` is which identical closing brace Git treats as context around the new output-extent branch; final control flow is the same. The substantive addition is `tests/smart_preview.rs:337-373`, the persistent-codec reopen integration test.

- `source.rs:132-162,172-181,199-203`: original recipe identity is separate from render/memo identity; equal identities are rejected. Original camera metadata survives; active dimensions come from payload pixels. The source keeps RAW semantics, has neither CFA nor working-RGB backing, and does not fabricate neutral metadata. Caller verification/content binding of the render ID remains an explicit boundary.
- `render.rs:574-576,613-625,655-663`: camera-linear dispatch precedes lens/CFA resolution; progressive levels retain their requested coordinates. Geometry uses the scalar pipeline, then ceiling reduction of cropped output for higher levels, retaining the reviewed odd-size rounding contract.
- `smart_preview_render.rs:11-52,82-110`: Native revision 2 and immutable prefix checks remain; retained calibration/WB, scalar local adjustments and geometry are used. SceneLinear remains floating-point working output; Display and DisplayLinear run their matching CPU output transform and retain requested tile coordinates. Duplicate coordinates are omitted and cancellation is checked before further delivery.
- `resident_render.rs` added guards decline resident metrics, tiles, lens paths, export rows, and surfaces before CFA resolution/destination mutation. `rgb_render.rs:75-79` refuses working-RGB layer rendering for proxies. Retouch, depth, lens blur and AI/depth masks still explicitly require original dependencies. No new route bypass appears in this commit.

## Added test assessment

`persisted_proxy_reopens_into_the_same_camera_linear_render_route` encodes/decodes a generated proxy, constructs a new `RawImage` with the original recipe owner and distinct render ID, applies custom WB/tint/exposure, and compares SceneLinear tiles at levels 0–2 against the scalar CameraLinear route of the decoded payload. It also checks retained orientation, original owner and absence of RGB backing. This meaningfully catches codec-to-image-core dispatch regressions.

The added test intentionally compares rendering of the decoded payload with its scalar reference; it does not independently measure pre/post-codec fidelity, verify caller render-ID derivation, or exercise physical file persistence. Those remain codec/storage integration responsibilities. Existing route tests cover display outputs, crop/progressive extents and unsupported-route rejection; their execution status is not established by this source-only review.

Earlier qualification limits remain: scalar work is not internally cancellable, progressive rendering repeats complete development per level, and this component does not enforce application-level original-only export or supply offline journal/reconnect/UI behavior.
