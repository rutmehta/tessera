# YuNet and SFace

Library only. Construct `FaceModels::load(&registry, SessionOptions::default())`,
then call `detect(&RgbImage)`, `embed(&RgbImage,&Face)` and
`analyze_and_store(&index,id,&RgbImage)`. All return Results; no hidden network
fetch happens during inference. Caller owns the registry/cache. Faces are in
input-image pixels, bbox xywh, landmarks right eye / left eye / nose / right
mouth / left mouth (subject's right, viewer's left). Orient previews before use.

YuNet: centered black 640x640 letterbox, raw BGR planar floats. Decode all twelve
stride-8/16/32 outputs with sqrt(clamp(cls)*clamp(obj)), exp width/height,
undo the actual rounded resize/padding, clip boxes and NMS at IoU .3. Default
confidence .9. Landmarks outside the image are rejected instead of distorting
alignment. `detect_with_thresholds` exposes both thresholds.

SFace: least-squares orientation-preserving similarity transform using all five
landmarks to the OpenCV 112x112 template. Bilinear inverse warp with black border.
Raw RGB floats, no external mean/scale (normalization lives inside the model).
Output is a validated, L2-normalized [f32;128]. Degenerate geometry errors.

## Identity clustering and incremental suggestions

`cluster(&[[f32; 128]], threshold: f32) -> Result<Vec<Vec<usize>>>` retains its
signature and complete partition/singleton behavior. It now uses **HDBSCAN**
(`hdbscan` 0.12, MIT OR Apache-2.0), not complete linkage or renamed DBSCAN.
The dependency builds a mutual-reachability MST, condenses its hierarchy, and
selects stable clusters. We supply a symmetric precomputed cosine-distance
matrix (1 - normalized dot product), minimum cluster size 2, min samples 1,
allow-single-cluster, and selection epsilon `1 - threshold`. Threshold is a
cosine similarity in [-1, 1]. A subsequent similarity-to-training-medoid gate
rejects distant members. It is **not** a minimum pairwise similarity guarantee.
Zero/nonfinite eligible descriptors and invalid thresholds error. Empty input
is empty. Noise remains singleton groups; every original ordinal occurs once.

`cluster_with_medoids(embeddings, threshold) -> Result<ClusterResult>` adds:
- `clusters: Vec<FaceCluster>` with sorted `members`, original `medoid_index`,
  normalized `medoid: [f32;128]`, and `eligible: bool` (false for noise/singletons).
- `eligibility: Vec<bool>`: input quality eligibility, all true in the ungated API.
- `approximate: bool`: true when the bounded sampling path was used.

A medoid is an actual member, not an averaged descriptor. It minimizes total
cosine distance using argmax x.dot(sum(y)), computed in O(n*128). Medoids are
recomputed after assignments, so the final medoid can differ from the training
medoid used by the threshold gate. Ties choose the first original ordinal.

**Large-catalog approximation:** up to 1024 eligible faces get full HDBSCAN.
Above that, a fixed-seed reservoir of 1024 trains HDBSCAN; unsampled faces join
the nearest eligible training medoid only above threshold, otherwise remain
singleton noise. Sampled noise stays noise. Runtime is O(1024²*128 + n*k*128),
k <= 512; distance-matrix storage is bounded at 1024² floats, plus O(n*128)
normalized descriptors/results. This is not exact full-catalog HDBSCAN. Rare
identities absent from the sample may be missed; density and input order affect
results. No claim of guaranteed identity recall or full batch/incremental
equivalence is made. Run a new batch to discover new identities, rather than
silently treating incremental threshold matches as confirmed identities.

`FaceQuality { confidence: f32, width: f32, height: f32, sharpness: f64 }` and
`QualityGate { min_confidence: f32, min_size: f32, min_sharpness: f64 }` gate
confidence, both box dimensions, and normalized face sharpness. Defaults are
0.9, 32 pixels, 0.1; configurable heuristics, not calibrated probabilities.
`gate.eligible(quality) -> Result<bool>` rejects invalid/nonfinite observations;
invalid gate configuration is an error. No blink/eyes proxy participates.

`cluster_eligible(embeddings, &[FaceQuality], threshold, gate)` returns the same
`ClusterResult`, preserving original ordinals. Excluded faces cannot train or
join identities. They remain ineligible singleton entries; if their descriptor
is unusable, their medoid is zero and must never enter identity matching.
`FaceModels::embed_eligible_faces(&RgbImage, &[Face], gate)` returns
`Result<Vec<Option<[f32;128]>>>`, measuring face sharpness before inference and
returning None for quality rejects. Existing `analyze_and_store` is unchanged.

`nearest_medoid(&[f32;128], &[[f32;128]], threshold)` returns
`Result<Option<MedoidMatch>>`; fields are `medoid_index: usize` (index in the
supplied medoid slice) and `similarity: f32`. Inputs are normalized/validated;
ties choose lowest index. None means no match. Callers must gate the new face
and pass only eligible/confirmed person medoids, map indices to persistent IDs,
and decide whether to offer a suggestion or create a person. No persistence or
confirmation happens in these APIs.

## Face signals and limits

Face sharpness uses ml-quality's normalized Laplacian variance inside the clipped
face box. `eyes_open` is only a weak five-landmark geometry plausibility proxy,
NOT eyelid aperture, an eye-aspect ratio, or a calibrated blink probability.
YuNet predicts one point per eye, so actual open/closed state is not observable.
The formula uses e=inter-eye distance, m=distance from eye midpoint to mouth
midpoint, n=distance from eye midpoint to nose, and a=absolute difference of the
two eye-to-nose distances/e:

    clamp(1-abs(e/m-.85)/.85,0,1) * (1-min(a,1)) * clamp(2n/m,0,1)

Degenerate e/m yields None. It is rotation/scale invariant but mainly measures
pose and landmark plausibility. A closed-eye face with the same landmarks gets
the same value. Never automatically label blinks or reject photos from this
proxy. It is stored for explicit review experiments, excluded from default
ranking and `FaceScorer`. True blink detection needs eyelid landmarks/a separate
licensed model in later work.

`Index::replace_faces` atomically writes face metadata, embeddings, per-face
`face/{ordinal}/sharpness` and optional `face/{ordinal}/eyes_open` score rows,
plus minima `face_sharpness` and `eyes_open`. Model provenance is
`yunet-sface-v1`. Empty results clear stale faces/scores; `faces_analyzed=1`
records completed processing even when there are no faces.
`FaceScorer::from_index` is a Send+Sync snapshot of minimum face sharpness,
with zero for missing data. cull's default live ranking combines quality with
face focus as quality*(.5+.5*face_sharpness), or uses the available signal alone.
Unscored members rank below scored members; all-unscored groups use file size.
Explicit scorers override this. Defect sweeps remain read-only and use the stored
signal names, including per-face names, with user-selected thresholds.

## Per-face UI strips

`face_strip(&RgbImage, &[Face]) -> anyhow::Result<Vec<FaceChip>>` computes UI
metadata from supplied detections, without model loading or downloads. It
preserves detection order. Each chip exposes:

- `crop_rect: [u32; 4]`: clipped source-image `[x, y, width, height]`, rounding
  fractional bounds outward. Crop the original oriented RGB image at this rect;
  chips do not allocate/store thumbnails or perform alignment.
- `focus_score: f64`: normalized [0, 1] sharpness of that exact crop.
- `eyes_open: Option<f64>`: the existing weak landmark geometry heuristic,
  **not** measured eyelid aperture or a calibrated blink probability. Degenerate
  geometry stays `None`; do not use this proxy to automatically reject photos.
- `person_id: Option<String>`: initially `None`, for caller-assigned identities.
  The catalog has named-person keyword predicates but no persistent face/person
  identity API; image-local face ordinals must not be used as person IDs.

Empty detections yield an empty strip. Empty images, invalid geometry/confidence,
and wholly out-of-image boxes return errors rather than partial results.
`FaceModels::face_strip(&mut self, &RgbImage)` detects with the loaded YuNet
session and delegates to the same function. It does not embed, write the catalog,
or fetch models during inference.

`face_strip_from_index(&index, image_id, preview_dimensions, identity_resolver)`
builds chips from cached face records. The resolver receives image ID and the
full FaceRecord (including its optional descriptor) and returns a host-owned
person ID. `frames_with_person_eyes_closed(&index, &image_ids, person_id,
threshold, identity_resolver)` returns unique input-ordered frames where that
person has a stored eyes proxy strictly below the threshold. Missing identities
and unknown eyes are excluded. Both APIs are read-only; neither infers identity
from a detection ordinal or writes a Decision. The filter is for review of a
weak proxy, not proof that someone blinked.

`cargo test -p ml-faces --test strip` exercises an explicitly supplied detection
on a generated face-like drawing, clipping/rounding, input order, empty/invalid
inputs, degenerate landmarks, and one-pixel crops with no model downloads.
This supplied-detection fixture does **not** claim synthetic face detection.
The separate model integration test compares the convenience method with real
detector output, without asserting that the drawing contains any detected faces.

## Runtime and model licensing

All weights are downloaded and SHA-256 verified by ml-runtime::ModelRegistry.
The manifest pins OpenCV Zoo revision 47534e27c9851bb1128ccc0102f1145e27f23f98.
Exact URLs, byte counts and hashes are in `../ml-runtime/models.toml`. Sizes are
comments because the existing strict ModelSpec schema has no size field and
runtime code changes are outside this work package.

- YuNet 2023mar: 232589 bytes, MIT model-directory license (not Apache).
- SFace 2021dec: 38696353 bytes, Apache-2.0 model-directory license.

Both satisfy the allowed-weight policy. Licenses reproduced under `licenses/`.
No InsightFace weights, no weights checked into Git.

The runtime image `run` API only handles single-output NCHW models. This crate
uses its Tensor/registry and an ORT adapter for twelve-output YuNet and rank-2
SFace. Session provider options match ml-runtime, with reported CPU fallback.
Partition integration tests use ml-runtime::Session::probe/partition_report
itself, require executed CoreML partitions, and report CPU partitions honestly.
They do not claim SFace is fully offloaded.

## Tests

`cargo test -p ml-faces --release` runs geometry, clustering, scorer, generated
face-pattern shape tests, RAW embedded-preview persistence and CoreML audits.
The pattern is generated in code, not a real person's image. No face count is
asserted for pattern/RAW fixtures. Four of five RAW fixtures currently expose
JPEG previews; no-preview files are skipped and at least one must be exercised.
Only network transport failures skip model tests. Corrupt hashes, manifest,
filesystem, inference and partition errors fail. Set `TESSERA_REQUIRE_MODELS=1`
to prohibit offline skips. Tests cache weights in ignored `.model-cache/`.

`tests/clustering.rs` verifies ARI > .95 against generated, labeled 128-D
identity populations with varying within-person dispersion (not a real-person
recognition accuracy claim), exact incremental-v-batch agreement on separated
identities, original-index quality exclusion, and deterministic bounded sampling.
The ignored release benchmark asserts 100k/100 identities under 30 seconds AND
ARI > .95. Run explicitly:

```sh
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-19
cargo test --release -p ml-faces --test clustering benchmark_100k_under_30_seconds -- --ignored --nocapture
```

`tests/multiface_cached.rs` never initiates downloads when cache files are absent;
it prints a skip (or fails with `TESSERA_REQUIRE_MODELS=1`). Override its cache
with `TESSERA_FACE_MODEL_CACHE`. Once cached, hashes/runtime errors fail normally.
It generates a two-face canvas, supplies known landmarks, embeds both crops with
real SFace, excludes a low-confidence crop, and clusters the two same-pattern
embeddings. It also executes YuNet, without claiming cartoon detection recall.
The older `tests/models.rs` intentionally still resolves/downloads weights.
