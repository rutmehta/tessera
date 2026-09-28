# EXP-45 phase 6: reciprocal auxiliary APP0/APP2 marker order

2026-09-28, same A host, macOS 26.6.2 arm64. Scratch-only diagnostic. Original A core ImageIO gate remains 4 pass/1 fail and was not modified or rerun as an acceptance gate. No Tessera source or tolerance changed.

`make_marker_order.py` pinned the original A SHA-256 `bb08f44d…` and qualified independent white Google16 explicit-denominator SHA-256 `61824f8e…`, parsed MPF auxiliary offsets/sizes and JPEG markers, then swapped only the two adjacent whole auxiliary segments in each isolated copy. A changed APP0 JFIF→APP2 ISO into APP2 ISO→APP0 JFIF over byte range 1438–1549; Google changed APP2 ISO→APP0 JFIF into APP0 JFIF→APP2 ISO over 1480–1591. Copy SHA-256 values are A `449195f44b39de0aa3e68bc7fa009a55ea98e3e9cf86016f5bbd59c0c7a23f28` and Google `1677ad705596932cf4e5c27ef4eac010aaaf6eb2f2012a79744277d762fbe4b7`. File sizes, primary bytes, MPF offsets, exact segment contents, ISO metadata bytes, ICC, and auxiliary compressed suffix remain unchanged. The whole segment positions necessarily change. Exact offsets/marker hashes appear in `*.swap.json`; `static_validate.py` independently checked content and MPF extraction, direct exit 0. Its initial assertion compared old/new segment offsets incorrectly; the failed diagnostic is preserved, followed by the corrected 0-exit check.

Pinned libultrahdr v1.4.0 (`d52a0d13814ca399fc8a07e23de1d2c63f0e8404`) decoded original/copy pairs to linear half-float HDR and sRGB U8 SDR. All eight decoder direct exits were 0. A original/copy HDR SHA-256 is identically `61cf27b4757daa75048dc3c4d44e4520838872ee7096e23df001442a91547aa5`; SDR `cd7ea07100f775c17d4b0385428ed2a750442ce93b7358d032e6f0c43422d205`. Google original/copy HDR `f8ed048c2b04f259ff1a65d77a3e35c17556d355c0322dd279a9d59d3240a19e`; SDR `bb0c4719ebc01d19663d6382bcd149db079c8c766775e523604eafe34c04d975`. Metadata text was identical within each pair. The initial decoder orchestration used an invalid SDR transfer argument and failed before qualification; that raw error is preserved. Corrected arguments matched the prior documented phase-5 invocation.

Same-run ImageIO and Core Image binary direct exits were 0. Raw ImageIO stdout retains its four `too few samples` prefix lines; parsed JSON is separate. Both source/copy pairs retain ISO aux recognition and finite/opaque samples:

| File | SDR peak | ImageIO HDR peak | ImageIO headroom | Core Image HDR peak |
|---|---:|---:|---:|---:|
| A original | 1 | 7.9837622643 | 8 | 16.000003815 |
| A APP2→APP0 | 1 | 7.9837622643 | 8 | 16.000003815 |
| Google255 original | 1 | 15.9515762329 | 16 | 16.000003815 |
| Google255 APP0→APP2 | 1 | 15.9515762329 | 16 | 16.000003815 |

**Result: negative.** Reciprocal auxiliary marker order alone does not transfer ImageIO's headroom behavior on this host. This neither identifies the residual A discrepancy nor passes its original ImageIO assertion. No split-gain or other follow-on variable was tested in this phase.
