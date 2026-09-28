# RES05a FFI snapshot preflight — SOURCE ONLY / UNRUN

Request f32843f7-d4d5-4bb2-9889-c7b0f5095da5 accepted after exact B target/expiry validation.
Branch codex/psd-copy-preflight starts at current fetched main345d6e11; no unaccepted Document/Save As ancestry.

Reviewed A dependency imported as source-only cherry-picks:
- 6b746bd4 = A186c9050 estimator/preflight product.
- 23e030f1 = A9e9692b3 field documentation.
- 7e735e04 = A142d1d6b mandatory native merged-alpha correction.
The resulting compositor/src/psd.rs and psd/src/compression.rs match exact A142d1d6b. A's estimator tests remain on its estimator branch, not duplicated here.

B tests-first commit a5197dfb; B product bcb1af3b. A can cherry-pick just those onto its reviewed estimator branch. No FFI public signature/UniFFI binding change.

RasterizedPsdCopyOperation.run_inner retains its immutable Arc<DocState> snapshot and existing extension/PSD geometry checks. It now routes that same snapshot through rasterize_copy: preflight_snapshot recursively counts Groups with checked usize addition; enabled smart-object filter stacks count as bakes; Pixel/Text/Shape/SmartObject leaves count as emitted rasters. Embedded smart-object child documents are not separately counted as output leaves. The estimator receives snapshot canvas/depth/channels.len and retained_merged_alpha=true because output remains Document::new(copy). Counting checks cancellation per layer. No image allocation/evaluator enters until preflight succeeds; only then does the existing collection/rasterization run. Encoding/tempfile/persist remain downstream and unchanged. No source re-lock or second document snapshot.

The estimate is modeled pixel payload, not measured peak RSS or a cap. No process budget, permit policy, bindings generation, GPU or scheduler changes. Operation admission, typed error precedence, cancellation and RAII drain are unchanged.

Four UNRUN tiny regressions cover:
1. Invalid saved-channel count, mandatory RGBA decoded-buffer limit and zero geometry reject with zero injected evaluator calls. Large extent is metadata only; child fixture is 2x2, no oversized image allocation.
2. Recursive group leaf/stack accounting, including disabled stacks and embedded smart child exclusion, matches reviewed estimator input.
3. Valid 2x2 evaluation occurs once, source remains smart, copy becomes pixel; cancelled snapshot never evaluates.
4. Real operation failure preserves a destination sentinel and releases admission; a fresh tiny valid operation saves a PSD signature, releases admission and allows preparation again. Actual encoder/tempfile path remains after the checked helper, not inside preflight.

Only source reads, edits and git diff --check performed on B. No cargo, formatter, compiler, test, app or benchmark executed; all test/strict/formatting/runtime claims pending A. Existing diagnostic histories and independent Layers APIs remain on their prior branches. B resource hold, paused heartbeat and desktop writer unchanged. A alone integrates/tests/merges main.
