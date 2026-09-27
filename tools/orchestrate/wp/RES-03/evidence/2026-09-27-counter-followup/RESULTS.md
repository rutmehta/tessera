# Partial masked-output counter follow-up — 2026-09-27

The combined integration candidate `55d1dea171fa4b5bbc1d94b0cb900103b03f36a4` passed 48 CPU tests and strict compositor Clippy, as recorded in the sibling `2026-09-27-combined` evidence. Review then found that `mask_tile_bytes_produced` was incremented only after all mask tiles and a cancellation check. A caller canceled after the first successful tile could therefore have 256 pixels visited and 4,096 bytes materialized while the produced-byte counter still reported zero. This follow-up changes accounting only: each successfully edited tile contributes its `Tile::byte_len` immediately. The pixel formula, pass admission, cache key, and publication rules are unchanged.

A deterministic private test uses a 257×1 F32 mask (one 256-pixel tile plus a one-pixel edge tile). Its after-tile checkpoint cancels the caller token after the first tile, with no thread timing or sleeps. It requires `EngineError::Cancelled`, 256 visited pixels, and 4,096 produced payload bytes. The production call uses a no-op checkpoint. The private helper simply extracts the existing per-tile blend loop so the test can cancel at this exact boundary.

| Gate | Exact command | Exit | Result |
|---|---|---:|---|
| Missing-helper RED | `cargo test -p compositor --lib partial_mask_cancellation_counts_only_completed_tiles` | 101 | Expected compile failure: `blend_mask_tiles` absent. This proves only the helper/test entrypoint was missing. |
| Behavioral RED | Same command, with helper using the old terminal whole-raster byte increment | 101 | Test compiled and failed exactly `left: 0`, `right: 4096`. |
| Final focused GREEN | Same command, with per-tile byte increment | 0 | 1 passed. |
| Masked-pass regression | `cargo test -p compositor --test smart_filter_deadlock` | 0 | 10 passed. |
| Cancellation regression | `cargo test -p compositor --test transform_cancellation` | 0 | 4 passed. |
| Strict Rust lint | `cargo clippy -p compositor --all-targets -- -D warnings` | 0 | Passed. Vendor LibRaw C/C++ warnings are retained in raw output. |

Every command used `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, `/Volumes/betterSSD/tessera-cache/target/main`, and a bounded subprocess timeout; none timed out. Source snapshots and manifests preserve the exact two RED variants and final GREEN source, including `Cargo.lock`. The temporary behavioral RED source was restored from the immutable GREEN snapshot after its expected failure. SHA-256 of the final checkout source and lockfile matched the GREEN manifest after restoration. `cargo fmt -p compositor -- --check` and `git diff --check` passed. No broader suite was rerun because the final source bytes were identical to those already used for the focused GREEN and strict lint gates. The earlier 48-pass evidence remains a separate result for the earlier candidate and is not claimed for this changed source.
