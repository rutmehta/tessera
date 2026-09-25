# style-profile (M3-09)

Library-local phase-1 base edits, without an LLM, generative pixels, or a new ML runtime.

## Host integration

1. Create `Profile::new(library_id, Questionnaire)` or `Profile::open(app_support, library_id)`.
2. Call `collect_library(&index, feature_provider)` to enumerate the entire catalog and read its real recipe sidecars. Only the latest User-authored state on the active history lineage is a label. Later unaccepted agent edits and abandoned redo branches are not training data. `collect` also accepts already-loaded recipes. Both retain one label per ImageId.
3. Supply cached SigLIP vectors (from ml-embed's VectorIndex rows), as-shot CCT/Duv, camera/lens identities and an unedited linear preview. `Features::from_perception(PerceptionInput)` computes percentiles, scene mean, clipping and face aggregates using ml-faces' `index::FaceRecord` boxes. Boxes must be in preview coordinates. Input RGB is linear Rec.709, not display-encoded RGB. Convert pipeline-cpu's scene-linear Rec.2020 output before this boundary. Existing inference/decoding stays with the host, so collecting a profile never downloads a model or silently substitutes fake measurements. Missing required perception should return an error from the provider.
4. Optionally `seed_references`: before/after pairs use the final after settings relative to engine defaults as labels. Before settings validate the pair, not a second subtraction from the target. Features always come from the source, not the edited preview.
5. `predict(&features)` returns a base DevelopSettings and 51 per-slider confidence/rationale records. Whitelisted outputs are WB, Basic tone, presence, vibrance/saturation, HSL and grading. Curves, LUTs, masks, detail, effects and geometry/crop are excluded. Applying a base preserves those existing fields. Numeric WB predictions use Custom mode, including default numeric values, so rendering cannot ignore them as AsShot or a preset.
6. Submit `BatchJob::new(profile, images, store, amount, timestamp_ms)` to the existing Scheduler. Priority is Score. The returned channel delivers the low-confidence-first review queue (ImageId tie-break). `SidecarStore` performs real atomic sidecar writes and advances vector clocks. Implement `RecipeStore` for another host persistence/Console surface. The in-memory `apply_batch` is transactional; persistent jobs commit per image and return committed queue entries on a later failure/cancellation. Scheduler status remains authoritative for overall success.
7. Explicit acceptance or correction calls `record_feedback(image_id, &source_features, &final_settings)`, then `save(app_support)`. Feedback replaces the image's old label, fits a new model transactionally, and does not learn from predictions automatically.

## Model and confidence

Pure-Rust centered PCA of L2-normalized embeddings, with up to 64 orthogonal components and zero-padding for low-rank libraries. It includes the complete vector, not its first 64 coordinates. PCA is refit with the labels and regression weights together on feedback, so weights never refer to a stale basis. Camera and lens identifiers use separate one-hot vocabularies. Numeric descriptors include logarithmic HDR statistics and an additional linear scene mean.

Each slider learns a range-normalized delta from defaults through ridge regression (lambda 0.01), solved with Cholesky. This version refits from retained labels rather than claiming an O(1) streaming update. For large libraries, collection batches labels and fits once. Confidence is a bounded heuristic based on sample support, per-slider training residual and distance to nearby observations, not a calibrated probability or a phase-2 rendered critic score. Questionnaire-only confidence is 0.1. Rationales disclose questionnaire versus learned associations and measured face EV deficits, without claiming causal attribution.

Burst components share WB and all predicted tone/presence sliders. Person components share exposure, texture and red/orange HSL treatment. Per-slider connected components combine overlapping burst/person constraints, so person exposure consensus cannot undo burst equality. Disagreement lowers confidence. This is global-slider skin treatment, not masked retouch or person-local skin editing. Full-strength base edits satisfy equality; fading toward different pre-existing edits naturally need not.

## History amount and required contract follow-ups

engine-api is unchanged. `HistoryGroup` currently contains only `id` and `name`, with no typed amount or rationale fields. Each run writes one named `Agent base edit` group and one Author::Agent history entry, including rationale. Neutral/zero-amount edits also record provenance. Per-slider rationale/confidence and fader data round-trip in the supported Recipe unknown-member extension:

`tessera_style_profile_groups_v1[HistoryGroupId] = { version: 1, amount, before, full, sliders }`

`group_amount(recipe, id).settings_at(amount)` defines the UI contract: absolute interpolation from before to full, 0 restores the exact prior settings, 1 uses the full base, and repeated fader movement does not compound. Intermediate amounts interpolate the numeric allowlist; WB mode switches to the full edit for nonzero amounts. UI code must replay later user edits above this state, not replace the entire current recipe. Undo/redo continues to use ordinary engine history.

A future engine-api change should add a typed group amount, baseline/full-strength parameter data, and per-slider rationale metadata. M3-09 intentionally stores these in a versioned extension instead of modifying the shared contract.

The index contains `recipe_hash` but exposes no public setter. `SidecarStore` does not mutate SQLite internals: the host must invalidate/rescan catalog hashes and render caches using committed review IDs, or implement that in its RecipeStore commit. The host must serialize this store against interactive writes; optimistic recipe comparison is not a filesystem compare-and-swap. Same-stem RAW/JPEG destinations are rejected, rather than overwriting another image's recipe. XMP fallback is read but base-edit output is the authoritative recipe sidecar, not an XMP export.

## Persistence and verification

`app_support/style-profile/<hex-library-id>.json` atomically stores version, library identity, questionnaire, samples, PCA basis, vocabularies, weights and residuals together. Missing files return None. Unknown versions, wrong library identities, malformed dimensions, invalid PCA and nonfinite values fail closed. Stable library IDs cannot traverse paths.

Tests use a synthetic catalog with 40 real recipe sidecars and 50 held-out feature rows. The fixed linear user style depends independently on scene mean and embedding coordinates, including a coordinate beyond 64. Additional tests exercise cached perception, PCA rank/serialization, questionnaire directions, overlapping consistency, history metadata/undo/redo, amount endpoints, a real Score scheduler sidecar job, cancellation, corruption, stale edits and filename collisions.

Run with CARGO_TARGET_DIR outside the checkout:

    cargo test -p style-profile --release
    cargo clippy -p style-profile --all-targets -- -D warnings
    cargo fmt --check
