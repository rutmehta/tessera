# Read-only saved-recipe depth histogram: test-first checkpoint

Status: test and test-seam preparation on `codex/depth-histogram-readonly` at starting main `3dbd4a47`. No production histogram refactor, build, test, native run, GPU, or GUI work was performed. The runtime/compiler lane is reserved; these tests have not yet been executed to record RED.

## Behavioral contract

`Engine::depth_histogram` must compute from the persisted recipe as observed by the call, without opening a `DevelopSession` or starting its `develop-save` worker. An already-open editor's unsubmitted settings must not affect the result. The call must not publish recipe, XMP, or index changes. Existing content-addressed depth cache hits remain usable, while an uncached request with no locally available depth model remains an error and performs no download.

The test seam is a `cfg(test)` counter owned by each `Engine`. It increments only after a Develop save worker starts successfully. This gives the behavior test an observable writer-construction boundary without a process-global counter or cross-test race. It is not an implementation allowance for production telemetry.

## Test coverage added

- `engine_histogram_uses_saved_pixels_without_starting_a_writer_or_publishing` opens one visible editor on a tiny indexed RGB JPEG whose disk recipe has a legacy Raw source tag. It pre-seeds deterministic depth cache rasters for both the saved input and materially changed live input, confirms those rendered inputs and expected histograms differ, then calls the Engine API. It requires the saved-input histogram, no extra writer construction, unchanged recipe/XMP bytes and unchanged index change head. It restores the visible editor's original exposure before closing because close may persist live state.
- `engine_histogram_keeps_the_cached_model_miss_error_without_downloading` makes a cold-cache request with no model installed and requires the stable missing-model error. The depth estimator's cache-miss policy disallows downloads.

The synthetic cached depth rasters avoid inference and model downloads. The same path exercises the saved process version and the RGB source-kind normalization required by current Develop open. Existing Develop initialization also installs renderer denoisers and mask hooks; an implementation must preserve those behaviorally relevant renderer inputs while removing writer/session construction.

## Intended implementation factoring after RED is captured

Extract the common read/setup path needed to load the indexed image and saved recipe, normalize source kind in memory, construct renderer plus denoiser/mask/depth dependencies, and build pre-geometry depth input for a saved process version. Keep the ordinary editor constructor responsible for owner baseline, editable state, listeners, save state, and worker creation. Route `Engine::depth_histogram` through the read-only context and the same depth provider/cache rules. Avoid a new exported API and do not acquire a writer lease in this slice.

The extracted path must preserve gate/catalog read ordering and revalidate the image ID-to-path mapping. The saved process version must reach the renderer, and the same mask cache hooks, CFA/RGB denoisers, and source-kind normalization used by visible Develop must remain available. A later lease implementation must additionally prove the histogram works while a lease for that same destination is held without acquiring another lease.

## Checkpoint boundaries

These tests are authored but unrun because the task explicitly withholds the compile/test/native lane. Do not claim observed RED or GREEN until the authorized runtime lane records command, source freeze, direct exit, and logs. No lease activation, FFI/generated binding change, Swift/UI work, or production refactor is included here.
