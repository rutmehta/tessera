# EXP-45 phase 7: isolated split-gain reference control

2026-09-28, A host macOS 26.6.2 arm64. Scratch-only, pinned Google libultrahdr v1.4.0 commit `d52a0d13814ca399fc8a07e23de1d2c63f0e8404`. The unchanged retained Tessera A JPEG and its original ImageIO 4-pass/1-fail gate are not patched or accepted. No production source or tolerance changed.

## One changed producer input

`make_split_pgm.py` pins the original 80×16 uniform-255 P5 gain PGM (`9d9293b6…`), retains the exact 13-byte header and all x=40–79 samples, and changes the 640 samples x=0–39 in each row to 0. New PGM SHA-256 is `9bf96ebb0b0f99632f21dd63f81e42cd78efaf4eef32e61be38370c26e079a55`. The exact mutation/counts are in `pgm-change.json`. The original PGM and all earlier fixtures remain unchanged. The pinned `/opt/homebrew/bin/cjpeg` (`95f541951b21ed95e41099e517b3e30eb0b666d36bee7f270c063b37b5de9c99`, libjpeg-turbo 3.2.0) used the same `-quality 100 -grayscale` options to make `gain-split.jpg` SHA `a01fa34e…` (direct exit0). `djpeg -grayscale -pnm` decoded it to the exact 0/255 step, direct exit0.

Pinned `build/ultrahdr_app -m 0` took the **same** `bright-white/base-bright255.jpg` SHA `86996ff7…` and `fixtures/cap-16.cfg` SHA `f5982114…`, substituting only `gain-split.jpg`, and emitted common-denominator ISO JPEG SHA `c797162d…` (direct exit0). `make_explicit.py` applied the identical, previously qualified ISO 0x48→0x40 denominator conversion, +24 bytes to APP2 and MPF auxiliary-size bookkeeping (direct exit0), yielding `reference-16-split-explicit.jpg` SHA-256 `eaeb9524ddfd2cee2d98c34512103ee7e06819471223de9caf1a8642c69c8968`. Static qualification found the primary image bytes identical to the uniform control except the MPF auxiliary-size field, with the same primary ICC and ISO values; the auxiliary compressed image and size necessarily differ. Extracted final gain JPEG includes encoder-added APP markers, so its whole-file bytes differ from `gain-split.jpg`, but the compressed scan bytes are identical. Its decoded 80×16 samples remain the exact step. The uniform final auxiliary decodes to all 255.

The pinned reference decoder emitted linear half-float HDR peaks of **16.0 for both** Google variants (direct exits0; command arrays and raw bytes retained). With split gain, its x=53,66,79 white-patch centers remain16.0; dark left and middle samples change as expected. Headroom metadata and ICC remain equal; independent ISO/MPF offsets and sizes are recorded in `static-qualification.json`. This qualifies a bright >8 control before native use.

## Same-host Apple comparison

The existing ImageIO and software Core Image probe binaries ran once each on the unchanged uniform Google white control, new split Google white control, and unchanged A JPEG. Both direct exits0; raw ImageIO stdout with `too few samples` diagnostic prefix is preserved separately from parsed JSON. All three ISO auxiliary images were recognized, and decoded pixels were finite/opaque.

| File | ImageIO SDR peak | ImageIO HDR peak | ImageIO headroom | Core Image HDR peak |
|---|---:|---:|---:|---:|
| Google white + uniform255 gain | 1 | 15.9515762329 | 16 | 16.000003815 |
| Google white + split 0/255 gain | 1 | 7.9837627411 | 8 | 16.000003815 |
| Original Tessera A | 1 | 7.9837622643 | 8 | 16.000003815 |

**Result:** The independent producer reproduces the A-like ImageIO half-headroom when its gain input alone is changed to this step, even though its exact-white pixels still have maximum gain and the reference decoder/Core Image reach16. This establishes content/codestream sensitivity in this controlled ImageIO path. It does **not** identify a specific Apple algorithm, show that all nonuniform maps fail, or prove the Tessera encoder correct. Base content/encoder differences remain; the original A ImageIO assertion still fails and was not waived. No further variable was tested.
