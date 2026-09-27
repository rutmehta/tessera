# Frozen gain-map core gate on A — failed

A macOS 26.6.2 (25G83), arm64. Source HEAD ae19820166b63aa7f75a6238add488513a5c8de6 adds evidence only to accepted product base69; all 13 uncommitted feature source hashes matched immutable snapshot 0c94c47d11d8375e07c4827ad0ae1c069981e6c5ede91786734409e166a853a6 before build. Exact command/environment in manifest.json: release export gain_map, existing /Volumes/betterSSD/tessera-cache/target/M2-45d, two jobs, deployment15.0, nocapture, one test thread.

Compiled successfully; combined command exit101 after113.131seconds, completed2026-09-27T18:44:13.865295Z. Five tests:4pass/1fail/0ignored.

Passed: canonical ISO/MPF association and independent/native five-patch reconstruction; privacy/native+XMP policy; unsupported settings/budget/cancellation/no-clobber; long XMP and gain-map source metadata deduplication.

Canonical native whole-frame vs independent reconstruction: max absolute0.125271575, mean absolute0.008142800, max normalized0.050658151. Original five-center4% criterion passed; no whole-frame4% claim.

Failed: gain_map_resize_and_sharpen_preserve_recipe_headroom, testline580, four-stop iteration. Independent base+gain reconstruction preceding native decoding passed. Native filtered peak7.9837623 vs expected16 failed unchanged4% bound. One/two-stop iterations passed. No host tests or retry occurred. Source remains unchanged; raw stderr includes ImageIO 'too few samples' messages after test summary.

Cause unresolved. Read-only SDK investigation identified possible diagnostic variables: CGContext.h667-672 documents target EDR headroom0 prevents tone mapping; test currently creates extended-linear-sRGB context with no explicit EDR target. CGImageSource.h248 documents image-specific luma scaling defaultsYES. Neither is established as cause. Proposed separate bounded diagnostic: retain exact1/2/4-stop source JPEGs; compare decoded image content headroom/provider samples with drawn pixels and default versus explicit target0, preserving original assertions and sources.

Heavy slot released after failure. B is reserved for its coordinator's B5-16 integration and remains untouched with prior dirty snapshot/capture evidence.
