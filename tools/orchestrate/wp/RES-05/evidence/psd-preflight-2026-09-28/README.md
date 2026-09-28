# RES05a PSD copy admission source and serial validation — 2026-09-28

Candidate `codex/psd-admission-estimate` HEAD `4f6b9751ae04eec6033306a0fe91378b0a7ba93e`, based on `eec9d14681648723fbc0ba04b221fe30886b9537`. Tests-only/native-alpha RED checkpoint `31de6e87` is an ancestor. `candidate-source.patch`, `red-source/`, and `green-source/` contain exact portable source bytes. The worktree was clean at every GREEN gate and after final validation. Main integration is owned by root; this package does not imply main was merged.

## Results

| Gate directory under `gates/` | Direct exit | Result |
| --- | ---: | --- |
| `red-native-alpha` | 101 | Genuine 1×1 native-alpha RED: prior preflight reached unsupported Fill layer export instead of rejecting 57 guaranteed composite channels. 1 failed assertion, no crash/timeout. |
| `green-estimate` | 0 | 4 estimator/early-preflight tests passed, including numeric-only 70 MP opaque RGB eligibility and native-alpha sentinel. |
| `green-psd-lib` | 0 | 21 PSD writer library tests passed. |
| `green-compositor-psd` | 0 | 9 adjacent PSD/cancellation/channel tests passed (3+5+1). |
| `green-ffi-preflight` | 0 | 4 new FFI pre-rasterization tests passed. |
| `green-ffi-copy-lifecycle` | 0 | 8 existing copy lifecycle/cancellation/commit tests passed. |
| `green-ffi-copy-transaction` | 0 | 4 existing IO copy transaction tests passed. |
| `green-ffi-host-copy` | 0 | 1 existing host precancel/retry/close test passed. |
| `strict-rustfmt-compositor`, `strict-rustfmt-psd`, `strict-rustfmt-ffi` | 0 each | Changed Rust sources format clean. |
| `strict-clippy-psd`, `strict-clippy-compositor`, `strict-clippy-ffi` | 0 each | Scoped release strict Clippy `-D warnings` passed. |

51 selected runtime tests passed (4+21+9+4+8+4+1), plus three formatting and three strict Clippy gates. Each Cargo command used the external cached release target, Cargo/Rayon workers 2, macOS deployment target 15.0, and a 600-second process-group watchdog. Exact argv/environment, full merged stdout/stderr, source HEAD/status/hash manifest and direct exit are preserved per gate. No full compositor/FFI suite, GPU benchmark, large raster stress, Machine B operation, or end-to-end GUI validation was run.

## Provenance correction

The runner used for `red-native-alpha`, `green-estimate`, `green-psd-lib`, `green-compositor-psd`, and the first `green-ffi-preflight` gate initially hashed tracked Cargo/compositor/PSD files but omitted tracked FFI files. Each gate recorded exact HEAD and clean pre-run Git status. For `green-ffi-preflight`, the preserved `provenance-note.txt` and `post-source-sha256.txt` document a **post-run** FFI hash audit; pre/post HEAD were both `4f6b9751` and pre/post status were empty. This proves tracked bytes correspond to the same immutable commit but is not mislabeled as a pre-run hash snapshot. The runner was corrected before subsequent FFI gates to hash Cargo/compositor/PSD/FFI tracked files both before and after and record post HEAD/status. Original records were not overwritten.

## Scope

The pure estimator reports a checked modeled pixel-payload weight. It is not an RSS measurement, allocation upper bound, hard process cap, budget, or concurrency permit. Hard early format rejection uses only guaranteed composite channels and the shared PSD encoder decoded-size rule. An imported opaque RGB document remains eligible when content-dependent alpha would exceed that rule; actual alpha is still checked after render. B's FFI hook counts enabled smart stacks and emitted raster leaves from the same immutable snapshot and checks before evaluator entry. No arbitrary threshold was introduced. Opaque metadata, off-canvas layers, masks, encoded vectors, allocator capacity, renderer/codec workspaces, GPU/OS allocations, and final output buffers remain outside the weight; resource incident closure is not claimed.

Independent root packaging verification checked all 13 GREEN direct-exit files and every available pre/post source hash against the frozen candidate checkout. Its first read-only verifier resolved relative manifest paths under the main checkout and stopped on an assertion; rerunning with the correct `render-resource-bounds` checkout base matched all hashes. This was a verifier path correction, not a source or test failure; no gate or source file changed.
