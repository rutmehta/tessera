# Fresh gain-map snapshot gate on Machine B — failed

Frozen uncommitted source: base 69bcd3e5d3d30ac334720ec6a02e1688b6f6b5cb plus immutable snapshot SHA256 0c94c47d11d8375e07c4827ad0ae1c069981e6c5ede91786734409e166a853a6. Six tracked changes and seven explicit new source/test files were byte-verified before compilation; initial dirty state matched exactly those 13 paths. Original wp/B5-16a ref remained bcd0e792f248b3d5ce20ea4a121b9c20e61c7d6c. No feature commit or other checkout mutation.

Host: Machine B M4 Max, macOS 26.1 (25B78). Existing ExifTool 13.55; existing Cargo target /Users/rutmehta/.cache/tessera-target/B5-15; CARGO_BUILD_JOBS=2; MACOSX_DEPLOYMENT_TARGET=15.0. No competing heavy build/test; existing yes/apps left untouched. Direct authenticated SSH execution by A, not a B chat receipt.

- `cargo test --locked -p export --release --test gain_map --no-run`: PASS, exit 0, 112.924 seconds.
- `cargo test --locked -p export --release --test gain_map -- --nocapture --test-threads=1`: FAIL, exit 101, 1 passed / 4 failed / 0 ignored.
- Passed: unsupported settings, byte budget, cancellation/no-clobber test.
- Failed: canonical ISO/MPF/reconstruction, native privacy, resize/sharpen/headroom, long-XMP/source-metadata tests. Each fails at gain_map_imageio.rs:96: expected a non-null ISO auxiliary dictionary; ImageIO returns null.

The canonical test successfully reached its native helper after independent MPF/ISO parsing, base+gain JPEG decoding, five patch-center reconstruction assertions, and ExifTool MPImage2 extraction. Native helper fails at auxiliary recognition BEFORE requesting SDR/HDR images or materializing pixel providers. Therefore this result proves native auxiliary recognition failed on this fresh output on B; it does not yet establish actual pixel decode failure, a malformed encoder, or unsupported macOS capability. No ImageIO skip or tolerance change was made.

Host tests were not run after core failure. No automatic repeat or artifact-retention mutation was applied. Test temporary directories were dropped, so no fresh JPEG/native pixel files remain from this gate; logs preserve the precise failure and frozen source is reproducible. Separate proposed diagnostic: retain the canonical JPEG before the native call, then compare its actual SDR/HDR decode with pinned independent Skia ISO controls on B. Keep original assertions and failed gate intact.

Evidence copied to A at /tmp/tessera-gainmap-b-fresh-gate: raw compile/core-test logs, process/thermal/OS snapshots, source manifest, exact patch and snapshot archive, run manifest, runner and ownership note. Nine remote evidence files byte-verified against remote SHA256 in sha256.json.

Finished 2026-09-27T18:31:08.920497Z. Ownership note records completed failure; B heavy slot released. Dirty B snapshot checkout intentionally preserved, A source remains frozen. No cleanup/reset, retry, host run, or production fix.
