# Depth histogram read-only test evidence

Tests-only source freeze: commit `77eb68d0aa32fa43d414ebb6522c3b6a34142307`.

## Intended RED

- Command: see `red-primary-77eb68d0/command.txt`.
- Environment and source SHA-256 values: `red-primary-77eb68d0/environment.txt` and `red-primary-77eb68d0/source-hashes.txt`.
- Raw compiler/test output: `red-primary-77eb68d0/raw.log`.
- Direct exit: `red-primary-77eb68d0/direct-exit.txt` = 101.
- The test completed all cache and saved-vs-live histogram assertions. It failed only because the per-Engine started-writer count changed from 1 to 2 during `Engine::depth_histogram`.

## Cache-miss control

- Command and source freeze: `control-cache-miss-77eb68d0/`.
- Raw output includes vendor LibRaw C++ warnings during its first build; the unit test itself passed.
- Direct exit: `control-cache-miss-77eb68d0/direct-exit.txt` = 0.
- The no-model cache miss returned `MISSING_MODEL_MESSAGE`; the path does not download weights.

## Target cache relocation

The first build used the worktree-local target cache. It was relocated after the runs because the internal disk had 3.1 GiB free. Both preserved BetterSSD copies contain 26,620 directory entries and 25,351 regular files; checksum-only rsync dry runs between the original internal tree and the first external copy, then between both external copies, exited 0 with no changed-file output. See `cache-relocation*.txt`, `cache-relocation-rsync-check.log`, and `cache-relocated-copy-check.log`.

- Verified copy: `/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0`.
- Relocated original: `/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated`.
- Worktree `target` symlink and future build path: `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated`.
- The target relocation is external to source history and is preserved as a symlink in the worktree.

No performance or GPU acceptance is inferred. Existing Develop backend initialization printed CPU/GPU L2 calibration timings as a test harness side effect. The tiny fixture invokes the current native rendering path, but no standalone GPU benchmark or GUI work ran.
