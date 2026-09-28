# Smart Preview export admission review

Approved: no actionable findings in `088cc261377ddb4dd710778a94997cdef010c437` relative to `a3ab5034`.

Reviewed all six changed files and the surrounding public export/Adobe call paths. All source-bearing single-export APIs (including enhanced and segmentation variants) converge on `render_one_cancellable`, whose first operation rejects CameraLinear. `render_pixels` independently rejects it before recipe, backend, segmentation, resize, or output work. Normal batch and the SR serial batch preflight every item before worker creation, preparation, publication, or progress callbacks; a later proxy therefore cannot leave an earlier original exported. RenderedExport has private fields and can only be obtained through the guarded render path. Original-copy export accepts an original filesystem path rather than RenderSource and is unchanged. Low-level pixel encoders likewise do not accept RenderSource.

All four Adobe render APIs converge on `render_linear_scaled_with_profile`, which rejects CameraLinear before profile validation and native rendering. Existing CFA and RGB behavior is unchanged by the new variant-only guards.

The three new export tests meaningfully exercise a real generated CameraLinearProxy through single/staged rendering, pixel rendering with upscale resize, and a mixed batch with an original first; they assert the explicit original-required error and no destination/progress. The Adobe test covers all four public wrappers. SR preflight coverage was verified by source inspection rather than a new model-dependent test.

Validation evidence inspected at `/Volumes/betterSSD/tessera-validation/smart-previews/export-admission`: final commit matches this review; export focused suite and positive batch/errors/export/staged/orientation suites passed; complete pipeline-adobe tests passed; release all-targets Clippy with `-D warnings` passed; changed-file rustfmt passed; before/after transitive-input hash comparison passed. Each corresponding recorded exit is 0. Historical compile-red and Adobe behavior-red evidence is retained. Validation used CPU export backend; no GPU or GUI runtime claim is made.

No source changes or builds performed during this independent review. Unrelated in-progress FFI/codec working-tree files were excluded from scope.
