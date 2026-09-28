# Editable Smart Preview native integration

Source commit: 81a6cc53 (codex/smart-previews), based on 6c057641. Fifteen Rust source/test files; no Swift or Document changes. Exact source hashes and HEAD accompany committed validations 12 onward. The source reviewer approved the bounded Engine API; subsequent production changes only collapse two nested dirty checks into equivalent Rust let-chains for strict Clippy.

## Behavior

Engine builds an atomically published local journal+camera-linear proxy from a pinned, verified copy of the original. Proxy Develop sessions preserve RAW recipe ownership, render on CPU, and save locally even with the original folder absent. Stable ImageId admission precedes canonical path admission; save workers retain their editor lease until drained. Original opening, direct recipe replacement and full-quality export refuse unsynchronized local edits. get_recipe exposes the local dirty recipe.

Synchronize requires closed editors and matching original length/digest plus conservative exact original sidecar baselines. A durable replay intent precedes original recipe/XMP publication. Both file and directory durability precede clean acknowledgement. Retry handles partial publication and stale acknowledged intents. Unknown envelope/root recipe fields survive; unknown nested owned fields including arrays are explicitly refused rather than erased. Divergence retains the local journal and original sidecars and reports Conflict.

Native original writers covered include Engine recipe/selection, Cull decisions and assist confirmation/history, metadata edits, agent run/accept/revert, Lightroom import publication, and people naming/undo/redo. Agent long-running mutations use non-mutex-held reservations. People publication verifies every actual member remains admitted. These are in-process admission guarantees, not cross-process locks.

## Evidence

- 03: focused Smart Preview suite, 22 passed.
- 06: real Sony public Engine workflow passed on a disposable copy; original fixture unchanged. Build, open, edit/save/close, hide copied original folder, restart Engine, reopen/edit/save, reconnect/sync, original editor switching, dirty/missing original export errors, and external-sidecar conflict.
- 06 rendered positive path: original full-quality JPEG before/after synchronized exposure edit is 4920 x 3276; decoded pixels differ and edited mean brightness increases. Separately, Original-copy export remains byte-identical to the source. These are distinct assertions.
- 08: full FFI unit suite, 171 passed.
- 09: related assist/changes/develop/export/library/lrcat/session integrations, 64 passed, 3 benchmark tests ignored.
- 12: committed strict Clippy for tessera-ffi and cull, all targets, release, -D warnings passed. Vendored LibRaw C++ warnings remain build-script output.
- cargo fmt --all -- --check and git diff --check passed before commit.
- Final committed unit/public workflow results are recorded in 13/14 evidence alongside exact HEAD and before/after source SHA256 manifests.

Earlier failures 01,02,04,07,10 are retained. They were compile/test-fixture or strict-check defects, fixed before the final evidence. 11 strict passed but its source-capture wrapper used the wrong JSON level and captured no sources; 12 repeats it with the corrected fifteen-file manifest. No failure logs were rewritten.

## Explicit limits

This accepts the native Engine path only. Actual macOS app offline Library relaunch still requires a separate indexed offline catalog/session API and UI integration. No full-app/native-binding/GUI acceptance is claimed. CPU proxies are explicit/offline opt-in: measured warm edits can be slower than original Metal, so Original remains the default. Proxy prefix supports Native2/denoiseOff and the current Detail tier (maximum 2560 edge); unsupported late RAW operations require the original.

Conflict comparison is deliberately conservative: external selection or metadata changes also conflict rather than automatically merging foreign-owned fields. Original and local dirty state both remain available. Local proxy saves do not regenerate original-keyed Library JPEG thumbnails; thumbnails can remain stale until synchronization/invalidation. No per-photo cancellation during synchronous build is provided. Status validates asset/original content and should run off the UI thread, not repeatedly per visible cell.

## Clean offline first-open follow-up

Root identified a missing first-open case after the bounded 81a6cc53 gates passed: a clean journal with the original absent caused get_recipe to synthesize a default recipe. Follow-up e1eca7ba changes only lib.rs and the public workflow test. get_recipe now uses the captured local recipe when dirty or when the indexed original is unavailable. It does not canonicalize/create the original directory. Clean+available original behavior is preserved.

The new workflow baseline uses exposure 0.25 with a matching history head and unknown recipe/envelope fields. Immediately after building, it hides the disposable original folder, restarts Engine, verifies OriginalOffline with dirty=false, asserts exact captured recipe JSON equality, opens/closes the clean proxy, and verifies the original folder remains absent. Existing dirty/reconnect/export/conflict coverage follows.

Run16 failed at test setup because exposure changed without a matching history head; run17 fixes that test setup and reproduced the actual clean-offline default-recipe defect. Both failure logs are retained. Final e1eca7ba evidence starts at18;81a6cc53 results are not relabeled as follow-up results.

Run18 on committed e1eca7ba passed the expanded complete workflow (5.72s test execution), including positive full-quality edited JPEG at 4920 x 3276. All fifteen source SHA256 values matched before/after. The scoped reviewer found no defect in the two-file follow-up. Independent post-run fixture SHA256 remains bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8.

Final e1eca7ba gates:19 full FFI unit suite171/171 passed;20 strict tessera-ffi+cull all-target Clippy passed;21 formatting passed. Each records the committed HEAD and fifteen unchanged source hashes. Git diff check passed and checkout is clean. Native compiler lane released after all gates exited.
