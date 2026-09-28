# Offline library validation

Production commit: 35e378b4. Test-only canonical-folder correction: 94557873. Feature checkout clean; no main merge by implementer. Sole compiler lane released after all commands ended.

Implemented Engine.open_smart_preview_library_session(folder) with explicit catalog-only declared asset membership, cached selection/metadata, missing-original FILE update retention, no original folder scan/canonicalization/stat during opening/listing/grouping/sync, no original dHash, and read-only cull/library/people mutation. Declared local nonempty pixels are not claimed pixel-validated; full validation remains on image open. Snapshot membership refreshes on reopen. All native APIs require the canonical catalog folder captured from index_folder(...).path while online.

## Evidence

Each numbered command has exact argv/environment, direct process exit, before/after full tracked+untracked nonignored source SHA256 manifest and equal freeze result. All cargo commands explicitly used CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated.

- 01 focused cull: 2 passed.
- 02 focused native offline declaration/filter test: 1 passed.
- 03 strict: failed items_after_test_module. Unchanged helper moved above tests.
- 04 strict after placement correction: passed.
- 05 full cull + tessera-ffi Release: 444 passed, 0 failed, 15 ignored across 51 result blocks; 351.9 seconds, at35e378b4.
- 06 actual Sony workflow: failed new offline library row-count assertion.
- 07 diagnostic: same failure, confirmed requested /var/folders/.../photos versus catalog /private/var/folders/.../photos. Native lexical contract intentionally does not resolve unavailable folders. Test corrected to capture index_folder(...).path while online, as required for Swift persisted offline paths.
- 08 actual Sony workflow at94557873: passed 1/1 (5.32s test,13.5s total). Offline library shows actual photo after restart; proxy edits render/save/reopen; original writes refused while offline; synchronize; full-quality JPEG4920x3276 has edited pixels; conflict and source-preservation checks pass.
- 09 final all-targets Release strict cull+FFI: passed.
- 10 final scoped format: passed.

Only the real ignored workflow test changed after full-suite gate; production inputs are identical, recorded by final-source-comparison.json. Its revised actual execution passed, so unchanged production full suite was not redundantly rerun. Ordinary test suite intentionally ignores opt-in fixture/model tests; only Sony Smart Preview workflow was explicitly enabled.

Original fixture SHA256 before/after: bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8 (16,646,144 bytes). Test operates on a disposable copy; original fixture preserved. The workflow's printed source hash is BLAKE3, not this SHA256.

No GPU/Compact patch applied, no speed/default-preference claim, no generated bindings or photo assets published. B must persist canonical index handle.path for offline reopen; this requirement and diagnostic were sent to root for UI review.
