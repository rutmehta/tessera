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
