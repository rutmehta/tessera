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

`cluster(embeddings, cosine_threshold)` is deterministic agglomerative complete
linkage: merge the most similar eligible pair of clusters, using minimum cross
pair cosine similarity. Input scales do not matter; zero/nonfinite descriptors
and invalid thresholds error. Returned cluster members are input ordinals, not
persistent person IDs. Complete linkage avoids transitive identity chaining.
This simple implementation is intended for small batches, not a full library.

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
