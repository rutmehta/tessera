# Saved-recipe depth histogram independent review

2026-09-28. Source-only independent review of tests-only commit77eb68d0aa32fa43d414ebb6522c3b6a34142307 in `/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera` (`codex/depth-histogram-readonly`). No test/compiler/native execution by this reviewer. Product factoring has not yet been reviewed or accepted.

## Initial tests and plan

The per-Engine cfg(test) counter increments immediately after successful develop-save thread spawn. It observes a real writer-construction boundary and avoids a process-global cross-test race. The success test seeds distinct content-addressed saved/live depth rasters, asserts rendered/model-input pixels and expected histograms differ, requires the saved histogram while a visible editor has unsubmitted changes, and checks recipe/XMP/index state before allowing editor close. This is a sound behavioral counterexample for the current Engine method's temporary session construction, subject to the owner's actual RED run.

The current missing-model regression preserves the expected no-local-model error. No network request is instrumented by that test; the no-download conclusion additionally depends on the existing CachedDepthEstimator code setting downloads_allowed(false). The test's isolated support directory is appropriate; external model-override environment remains a general fixture precondition.

The legacy Raw source tag on an RGB file exercises compatibility but is not a discriminating pixel oracle for source_kind normalization: the current depth_input uses RawImage/settings/process_version, not the recipe.source_kind field. Do not claim this test alone proves that assignment survives factoring. Shared loader design and source inspection must preserve it.

## Additional regression requested before GREEN

A saved unsupported process version must be tested through Engine::depth_histogram, preferably with the normal-input depth cache preseeded, to prove the read-only path rejects unsupported saved process rather than silently reconstructing native-process pixels or returning a cached result. Existing Adobe99 process tests exercise session APIs, and the new success fixture saves the default process, so neither catches that new Engine-path regression. Owner acknowledged and will preserve the current frozen RED before changing tests.

## Required production factoring boundaries

- Keep initial catalog path lookup, release it, acquire recipe read gate, reacquire catalog lock, revalidate image-ID-to-path mapping, and read persisted recipe in that order. Avoid lock inversion.
- The ordinary editor's recipe and OwnerBaseline must still be captured in the SAME read-gated/catalog-locked critical section. Do not read recipe in an extracted helper and then obtain a potentially newer owner baseline after releasing the gate.
- Normalize recipe.source_kind from actual decoded RawImage source in memory; no recipe/XMP/index publication by histogram.
- Read-only state uses saved recipe/settings, never a visible editor's unsubmitted settings or session display/crop flags.
- Preserve session_renderable behavior and exact existing pre-geometry depth-input reset: effects/geometry defaults, lens distortion_scale/manual_distortion zero, remaining lens inputs unchanged, ≤1024 long-edge level selection, scene-linear output and saved for_process_version.
- Preserve renderer_snapshot depth provider, explicit/automatic CFA denoisers, RGB denoiser, model registry, and mask cache hooks. A reduced renderer that accidentally drops these adapters can pass the plain RGB test while changing real recipes.
- Ordinary editable state, owner-baseline mutex, listeners, save state/condition variable, worker and close machinery belong to the editor constructor. A fake no-worker flag that still constructs Shared/State/session write machinery does not satisfy the intended read-only factoring, even if it passes the counter test.
- Keep content-addressed depth estimator/cache behavior and no-download policy. Current Engine API may populate appropriate cache/model infrastructure; 'read-only' here specifically excludes writer-session construction and recipe/XMP/index publication.

No lease activation, FFI/generated binding changes, Swift/UI edits, B-owned work or full acceptance is covered by this review.

## Pre-implementation factoring proposal review

Owner proposed one recipe-open helper returning path, Recipe and OwnerBaseline from the existing same critical section; histogram discards the baseline, editor consumes it. A separate read context owns only RawImage, renderer/mask hooks, model registry and denoiser/depth adapters. It renders explicitly supplied saved settings/process version, without constructing Shared, State, listener/save/worker machinery. This architecture has no conceptual blocker and preserves the relevant snapshot boundary.

Precision requested before implementation: keep existing catalog-resolved path semantics, without introducing filesystem canonicalization/symlink changes; retain RawImage decode and expensive renderer setup outside the gate/catalog critical section; extract common renderer-snapshot and pre-geometry depth-input helpers rather than copying nominally equivalent branch trees. In particular preserve explicit CFA override, automatic CFA selection and RGB fallback precedence plus depth provider installation. Read-only context begins with no explicit per-session CFA override, matching the old temporary-session path, and must not borrow a visible editor's transient configuration. The source checkpoint, tests-only unsupported-saved-process follow-up, and actual RED/GREEN evidence remain pending owner execution.

## Tests-only follow-up b66d0348 review

Reviewed exact `b66d0348b923aed701810bc9673f5bb34c8176f6` added Adobe99 Engine regression. The fixture seeds a valid normal-input depth cache, writes an unsupported saved Adobe revision, and asserts both a specific process error and unchanged writer count. This appropriately discriminates a silent-native-process/cache shortcut once the error oracle is corrected.

Pre-run issue found: the assertion expects `unsupported process version`, but the actual depth renderer at `crates/image-core/src/render.rs:420–425` returns `EngineError::invalid("process_version", "Adobe PV3–6 required")`; EngineError display formats it as an invalid-argument error. The expected phrase exists only in a different Develop setter. Requested exact process-version/Adobe-range error matching so the run fails at the meaningful writer-count boundary instead of a mistaken error-text oracle. Owner/root notified before runtime acknowledgment. No test execution by this reviewer.

Read preserved original RED raw log: test passed saved/live input/histogram comparisons and failed intended writer count2 versus1; missing-model control is separately recorded direct0. These are owner's executed results, not new reviewer execution.

## Corrected test and exact production source review

The test-only correction `c2737914525eeb6e59b14ca173cd298304932201` now matches the actual `Adobe PV3–6 required` renderer error. No remaining source blocker was identified in that regression fixture or its writer-count oracle.

Reviewed exact production commit `a04a4e71bbd5448d31591a524891141faece138d` by diff against its tests/evidence parent. No source blocker found; authorized runtime owner was notified before GREEN testing. This is source clearance, not runtime acceptance.

The new Engine::depth_histogram path captures a disk snapshot, opens RawImage, normalizes source_kind in memory, creates DevelopRenderResources, and renders saved recipe settings with the saved process version. It never constructs Shared, State, DevelopSession, listener/save state, writer thread or close machinery. The unused captured OwnerBaseline is merely dropped as data, so this is not a no-worker flag around full session construction.

`develop_disk_snapshot` preserves initial catalog path lookup, gate read acquisition, reacquired catalog lock, ID-to-path revalidation, and recipe plus OwnerBaseline capture within one critical section. RawImage opening and renderer/model setup remain outside that section. Existing path semantics are unchanged.

`develop_render_resources` preserves renderer choice, per-image mask hooks, model registry, explicit-CFA slot initialized empty, automatic-CFA cache, RGB denoiser and DepthProvider. Shared::renderer_snapshot and the read-only resources both call the same extracted helper, retaining depth attachment and explicit-CFA → selected automatic-CFA → RGB fallback precedence. The common `render_depth_input` preserves exact effect/geometry reset, the two lens distortion zeros, ≤1024 long-edge level choice, SceneLinear rendering with explicit for_process_version, and tile-to-image assembly. Existing live session depth_input still captures drawn state plus its recipe process version and delegates to this same helper; visible editor semantics are preserved in the reviewed diff.

Focused regression and relevant adjacent gates still need the owner's actual direct results, frozen input hashes, and validation on the intended integration tree. This source review does not establish lease behavior, all-writer coordination, performance, GUI, or full product acceptance.

## Final checkpoint and bounded review disposition

Final implementation source is `a04a4e71bbd5448d31591a524891141faece138d`. Final tested source checkpoint `a31d25185f054cc95647b8ff7f8efc4f7a3b5167` differs only in seven documentation-comment lines that move the editor-opening description back to open_develop_session and describe histogram's saved-recipe/no-session behavior. Independently inspected this delta: no executable behavior change. Final evidence commit is `6be9d5d7`.

Independently read the preserved raw logs/direct-exit files: the original saved/live regression has intended RED writer count2 versus1; corrected Adobe99 regression has the same intended RED after passing the process-specific error assertion. Focused three regressions PASS/direct0 at a04. Adjacent seven tests PASS/direct0 and cargo fmt check exits0 at a31. Strict `cargo clippy -p tessera-ffi --lib --tests -- -D warnings` finishes/direct0 at a31. No native/compiler/test execution was repeated by this reviewer. Root additionally reports a1176-file tracked Rust/Cargo snapshot exactly matching a31; that broader snapshot was performed by root, not repeated here.

Disposition: no source blocker remains for integration of the exact validated source/test/evidence slice. The change removes real temporary session/worker construction and shares existing renderer behavior instead of bypassing it. The focused and adjacent results support this bounded saved-recipe histogram change. No whole-suite, lease enforcement, all-writer coordination, crash durability, FFI/Swift/UI behavior, inference-quality, memory/performance, or GUI acceptance is claimed. Existing unrelated failures and ownership boundaries remain unchanged.

This review is now frozen for coordinator import.
