# M3-02 implementation handoff

Result: PASS. Required command passed (35 tests, zero failures), strict
Clippy and workspace formatting passed. Additional `cargo test -p index --release`
and `cargo test -p ml-runtime --release` passed. Their existing manual benchmarks
remain ignored. No commits, no engine-api/CLI changes, no local target directory.

Implemented YuNet detection/NMS, five-point SFace alignment/128-D embeddings,
complete-link cosine clustering, face focus and explicitly weak eye-geometry
proxy. Implemented documented normalized classical quality measurements and
score writers/Scorer snapshots. Migration 005 stores faces/embeddings; atomic face
replacement maintains per-face and aggregate score rows. Default cull ranking
reads real persisted signals, while defect sweep remains review-only.

Weights fetched by the registry and verified from downloaded bytes:
- YuNet 2023mar: 232589 bytes, SHA-256
  8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4 (MIT).
- SFace 2021dec: 38696353 bytes, SHA-256
  0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79 (Apache-2.0).

Both registered as revision 1. Upstream filenames/dates, pinned commit URLs and
sizes documented in models.toml. Byte sizes are comments to preserve existing
strict ModelSpec schema. Licenses reproduced in ml-faces/licenses. Weights cached
only in ignored ml-faces/.model-cache, not part of the patch.

Real model tests ran with TESSERA_REQUIRE_MODELS=1 (no offline skips): generated
pattern shape/embedding checks; four available RAW embedded JPEG previews wrote
scores; CoreML partition audits through ml-runtime. YuNet: 1/1 executed partition
CoreML. SFace: 28/58 executed partitions CoreML, remaining CPU. The optional
ml-runtime TESSERA_REQUIRE_COREML=1 all-node policy would reject hybrid SFace;
no claim of full SFace offload is made.

Limitations documented in crate READMEs: five-point eye proxy cannot identify
blinks, anisotropy is not calibrated motion-blur probability, noise may have no
flat support, and quality score upserts are not a multi-row transaction.
An independent read-only review found no blocking correctness/security issues.
Its optional recommendations: atomic quality batches, reference detector outputs,
and more combined/default ranking branch tests.

All changes are confined to the user-allowed paths. Build artifacts stayed in
/Users/rutmehta/.cache/tessera-target/M3-02. See verification.log for the requested
command's test output (pre-existing vendor C++ warnings omitted).
