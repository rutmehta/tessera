# EXP-45 phase 5: reciprocal primary ICC profile diagnostic

The isolated source JPEGs and output copies are documented in `RESULTS.md`. The 588-byte ICC profile payload was the only changed byte range in each copy. `A-swap.json` and `Google255-swap.json` identify the exact offsets and hashes. `static-validate.log`, the eight reference-decoder records, and `native/` preserve direct outcomes and raw diagnostics. ImageIO still reports A at headroom 8 and the independent white control at headroom 16. This negative result does not pass the original A ImageIO gate. No Tessera product source changed.

`MANIFEST.json` covers the isolated phase; `SHA256SUMS` covers this portable bundle. Pinned libultrahdr source and compiled binaries are excluded; their hashes and provenance remain in the earlier external phase manifests.
