# M2-49 implementation plan

Spec: brief.md and the user's stricter path allowlist. No commits or pushes.
Implementation uses test-first vertical slices, preserving observed red/green output.

- [x] Resident composed geometry implementation (full latency acceptance remains open): extend the existing LensPlan inverse map with Upright, share plan resolution in interactive rendering and previews; test L2 parity and measure warm edit latency.
- [x] Guided session uncorrected option: bypass composed geometry only for overlay rendering; preserve saved recipe.
- [x] Automatic CFA noise: estimate canonical site variances from spatially flat raw patches, retain explicit calibration override; install identical lazy adapters for render/export.
- [x] Model downloads: registry progress with explicit download policy, FFI queued/downloading/ready/failed listener callbacks; checksum-verified atomic cache installation.
- [x] Cached depth: per-image content/model cache, render/export hook, missing-weight export warning, histogram/visualization and subject focus; retain focal-range semantics and cover aperture shapes.
- [ ] Regenerate bindings and run requested release-test/clippy/fmt/Swift gate; write REPORT.md with exact measured outcomes and remaining limitations.

Review focus: cache invalidation after edits; invalid geometry; no-weight/no-network behavior; CFA site rotation; depth alignment through crop/warp.

Bindings and Swift compilation are complete. The exact gate was attempted; see REPORT.md for the failed export/slider concurrency gate, maximum-frame latency failures, cold Auto cost, and missing-weight verification limits.
