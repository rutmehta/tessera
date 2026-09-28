# EXP-45 x32 boundary-shift preparation: rejected before execution

Decision: do not run the prepared x40→x32 control. It is not sufficiently isolated to identify the proposed base/gain edge-registration cause.

The prepared script is preserved at `/Volumes/betterSSD/tessera-validation/exp45-boundary-shift-2026-09-28/preflight/prepare_shift_x32.py`, SHA-256 `9c3b6109ec678aeb9b5daad014d6e82248f0760337baf70bff5da2af300005c6`. It is an unrun draft, not an accepted experiment or validated builder. No cjpeg, reference decoder, ImageIO, Core Image, app, or native process was run by this preparation. The original A file and acceptance gate were not modified.

Two confounds defeat the intended inference:

1. Moving a single 0/255 vertical step from x=40 to x=32 changes the gain histogram from 640 zero / 640 255 samples (50/50) to 512 zero / 768 255 (40/60). A changed peak could reflect gain-area distribution instead of spatial registration.
2. The draft encodes the changed map with pinned libjpeg-turbo `cjpeg`, whereas A's gain map comes from Rust `jpeg_encoder::Encoder` at quality 100/Luma (`crates/export/src/gain_map.rs`). A changed result could reflect auxiliary JPEG coding differences. Phase 7 shows cjpeg can independently reproduce the ~8 result, but that only proves A's encoder is not necessary for that behavior; it does not make a cjpeg-reencoded A-base variant an encoder-controlled comparison.

The repacker draft attempts to preserve A's primary bytes, ICC, ISO payloads, and MPF association (patching only the necessary MPF auxiliary size in the primary stream), but that structural effort cannot rescue the causal ambiguity. A paired cjpeg x40/x32 set would remove the encoder delta *between those two variants* while leaving the 50/50 versus 40/60 content change; it would expand the experiment without establishing registration as the cause. No runtime is selected from this evidence.

The phase-7 control remains relevant: its base has edges at x=26 and x=53 while its gain-map edge is x=40, so it is not an exact aligned-edge control. The phase-7 and phase-10 outcomes remain as recorded in `tools/orchestrate/wp/EXP-45/evidence/2026-09-28/reference-control-phase7/RESULTS.md` and `tools/orchestrate/wp/EXP-45/evidence/2026-09-28/context-headroom-phase10/RESULTS.md`. The original ImageIO gate remains failed (4 pass / 1 fail); no threshold waiver or product repair follows.

Conclusion: no justified isolated next runtime experiment is selected. A useful next investigation requires a design that varies registration while controlling map distribution and auxiliary coding, or a vendor/normative contract specifying ImageIO output for nonuniform ISO gain maps. The present one-edge x32 proposal does neither.
