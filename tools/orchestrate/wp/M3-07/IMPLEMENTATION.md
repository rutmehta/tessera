# M3-07 implementation

Implementation is in `crates/cull/src/learning.rs`, `learning/pca.rs`,
`learning/review.rs`, and `crates/ml-faces/src/strip.rs`.
Public integration instructions and limits are in the two crate READMEs.
No dependency additions, index migration, engine-api or Selection changes.

Acceptance coverage:
- 43 bounded features: 11 technical/face/context + 32 per-library PCA values.
- Online logistic updates, cold-start priors, signed feature explanations.
- Versioned, library-isolated atomic persistence of weights and frozen PCA.
- Assisted queue ordering and explicit automated suggestion confirmation.
- Learned/technical best-of-burst blend with sharpness tie breaks.
- Face-chip extraction from supplied detections, loaded detector, or catalog.
- Identity-resolved per-person low-eyes-open review filtering.
- Synthetic blur rule: 100/100 held-out predictions after 30 labels,
  compared with 85/100 using the cold-start learner.
- Tests for stale/invalid batch rejection, no pre-confirm writes, undo, cursor
  preservation, corruption, PCA rank/variance recovery, and clipped face crops.

Limitations are explicit rather than hidden:
- Five-point eyes-open is a geometry heuristic, not verified blink detection.
- Person IDs are supplied by the host resolver, never inferred from face ordinals.
- Application Support root, stable library ID, analyzed-preview dimensions and
  embedding vectors are host inputs. Model save errors must be surfaced/retried.
- Undo restores Selection, not historical online training events. Rebuilding
  from final user decisions is the exact-retraining option.
- This is a library API work package, not new engine-api/FFI/UI wiring.

`verification.log` records the requested release tests, clippy -D warnings and
workspace fmt check. All Cargo commands retain the external CARGO_TARGET_DIR.
