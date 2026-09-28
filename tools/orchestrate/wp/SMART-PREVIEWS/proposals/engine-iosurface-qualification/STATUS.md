# Future GPU qualification — UNRUN

Source-only actual Engine/IOSurface harness reviewed at patch SHA25648f95e6fe727af9a676b8a4ed876d00b530d600cc7ffeccf587633a5b3c135ef. Apply only after the separately reviewed consolidated GPU candidate and after current thumbnail/Swift gates. No GPU candidate or this harness has been applied or executed at preservation.

Measures surface-delivery latency, not physical display/input-to-present latency. Fresh process does not mean cold filesystem cache. Forced routes prove capability separately from auto-calibration. Per-edit Metal counters and resident surface receipts are required; no silent CPU fallback. Pixel normalization/readback runs outside timing, and same-dimension comparisons avoid conflating adaptive quality with speed. Numeric fidelity tolerances are provisional, not a perceptual-quality acceptance. Original remains default.
