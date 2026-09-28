# Stage C native checkpoint — independent review

Reviewed exact source `4f26ef9528fcbfecd1f208975309e84e2d48c30c` in `/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera`. Review was source-only: no test, compiler, native probe, GUI, or product edit in this lane. Runtime evidence remains pending at this note's creation.

## Source clearance

No remaining source blocker found for focused/adjacent validation of the bounded, participating-Engine lease contract. This is clearance to validate the candidate, not a claim that its tests passed or that physical-file aliases are fully exclusive.

The initial implementation at `92ad13b63331960cb19eb82f4c430e9eddc9796a` reserves an owner ID and captures the saved recipe/baseline inside destination gate → catalog/path validation. On snapshot error it clears tentative ownership under the existing guard rather than dropping a token that reacquires that mutex. The returned owning token remains local through image decode/resource setup/spawn failure and transfers into session ownership on success. Snapshot/decode run without keeping the short gate held across decode or rendering.

Session and save-worker closure each own a reference to the same lease reservation. Shared retains only non-owning gate/owner authority, avoiding lifetime coupling to rendering. Successful explicit close flushes, joins the worker, closes rendering, then takes/drops the session lease even if a closed session Arc remains retained. Failed close leaves the reservation intact. Final session Drop on its own writer keeps the worker's lease reference alive through loop return, preserves self-join avoidance, and drops that reference before the test-only exit notification. No worker waits while holding the destination/catalog guards in the reviewed code.

Full save and auxiliary repair resolve the current gate, then authenticate the captured gate Arc identity, owner ID, and destination key before taking catalog and rechecking image ID→path before publishing. Thus resolving a new gate does not silently confer the old authority on it. The direct recipe setter uses lease-rejecting begin_write; selection uses the explicitly allowed begin_selection_write and still advances the independent write epoch. Owner IDs use a separate checked allocation sequence. Release clears only a matching owner ID.

## Reviewed test corrections

`6352ef1a10f4eacc56c8c2ca9382fd2b0079985e` and `59210c1a38a95584bb67bd456656bc16dcd78ca4` close the earlier coverage gaps:

- second admission is rejected before the second Engine's post-spawn writer count changes;
- saved histogram constructs no writer and still leaves a competing editor rejected;
- malformed snapshot and later missing-image decode are separated by a successful reopen, assert diagnostic phase/fixture path and unchanged worker count, then restore/reopen;
- failed close retains admission until actual repair/drain succeeds;
- closed A remains retained when B opens, repeated close and final Drop of A cannot release B;
- stale authority, a different gate, and a different key on the same gate each reject while preserving B's valid authority;
- close-in-flight rejection pauses in the saved callback after Engine save returned and dropped its destination gate; bounded contender result and unwind release avoid the original held-gate deadlock proposal;
- final session Arc destruction is explicitly checked for writer ThreadId and strong_count==1, followed by Weak session disappearance, continued rejection while callback is paused, actual worker completion, and successful B admission while an explicit old Shared Arc remains held;
- the older lock-contention control now holds begin_read rather than a forbidden direct-write guard, preserving its original gate→catalog-progress assertion.

The new callback fixtures use bounded waits and release-on-unwind guards. The worker notification follows the owning worker lease drop in ordinary code; it does not release ownership only in test builds. This review does not infer an observed test result from those source assertions.

Both root and this reviewer caught an image-ID move/use compile error at `59210c1a` before any execution. Keep that checkpoint explicitly UNRUN, not a recorded compiler failure. `4f26ef95` changes only three lines in the close-in-flight test: clone `contender_id` before the move closure and use the clone there. The final reopen retains the original ID. The exact diff was independently read; it introduces no product changes.

## Scope and known limitation

The implemented identity preserves the pre-existing `destination_key`: canonical parent directory plus exact recipe filename, including existing same-stem destination collision semantics. The coordinator's separate filesystem investigation established that case-variant spellings can alias one physical file on the current macOS filesystem while producing different keys. No key normalization was added here. This candidate therefore must not be described as exclusive across every spelling or every physical-file alias; that existing gap needs separate bounded treatment before a stronger claim. This review did not execute or independently reproduce the filesystem probe.

The fence covers the declared in-process Engine writers and Develop full/repair saves. It does not serialize all Agent/Cull/import/merge or external filesystem writers, provide filesystem CAS, create a multi-file transaction, or establish global Quit/crash durability. OwnerBaseline's foreign-field protection remains separate. Nested unknown owner fields remain fail-closed rather than preserved through serialization. No B Document/Save As or performance acceptance is included.

## Runtime follow-up

Root granted the native owner one serial focused/adjacent/format/strict lane at exact `4f26ef95`, with 1177 source-input freezes. When saved results arrive, this reviewer will compare direct exits, actual selected counts, warnings/failures, and before/after hashes without rerunning tests. The earlier d615 admission REDs and foreign-disk control remain separately preserved.

## Observed compile failure and corrected checkpoint

After the source-only clearance above, actual execution at `4f26ef95` stopped at compilation: four E0433 references to missing module `thread` in the new test module, direct exit 101, no tests run. This reviewer missed the import during source preflight. I independently read the preserved raw diagnostics and verified byte-identical before/after source manifests in `compile-harness-failure-4f26ef95/focused-second-editor/`. This is an observed harness compile failure, distinct from `59210c1a`'s move/use error caught before any execution.

The corrected exact checkpoint is `096873b2f85698c7743045a2155134effe986d00`. Its diff from `4f26ef95` is solely `use std::thread;` inside the cfg(test) module; no product source changed. I independently verified all 1177 entries of its focused manifest against Git blobs at that ref: all tracked Rust files, Cargo.toml/Cargo.lock files, and rust-toolchain.toml, with no missing Rust/Cargo inputs. Subsequent saved groups are being compared byte-for-byte against that verified baseline. Runtime completion will be recorded below only after the final commands settle.

## Final source and saved-evidence acceptance

Final validated source: `593730668d6bcc5f65b8ea88b2febc999393e1f1`. The entire source delta from `096873b2` is the mechanical nested-if → let-chain rewrite in `LeaseReservation::drop`. Evaluation still acquires the epoch/destination guard first, keeps its named binding alive through the owner-lock acquisition and matching-ID check/body, and short-circuits without touching the owner on failed epoch acquisition. It does not release the gate before clearing the matching owner or broaden release to stale IDs.

The intermediate `096873b2` run passed 34 test invocations covering 31 unique named tests and formatting, but strict Clippy exited 101 for `collapsible_if` at `recipe_write.rs:271`. That failed strict result is retained. One historical histogram filter selected zero tests; it is not counted as coverage, and the corrected fully qualified filter later selected/passed the intended test. No strict success is claimed at `096873b2`.

I independently audited the saved final results in `tools/orchestrate/wp/UX-03/evidence/develop-exclusive-admission-tests-2026-09-28/runtime-59373066/`:

- Eight focused commands each ran one test and returned direct exit 0: second editor, direct setter, failed open, failed close/repair, close-in-flight, final worker Drop, ID exhaustion, and stale authority.
- The recipe-gate group ran 10 tests, all passed/direct 0. Two overlap the focused set, so these selected groups cover 16 unique tests rather than 18.
- `cargo test -p tessera-ffi --lib -- --test-threads=1` ran 137 tests: 137 passed, 0 failed, 0 ignored, 0 filtered; direct exit 0. This includes the selected cases. The final evidence contains 155 test invocations in total, covering 137 unique names, not 155 unique tests.
- `cargo fmt --all -- --check` and `cargo clippy -p tessera-ffi --lib --tests -- -D warnings` both returned direct exit 0 at the final source.
- Every final group's before/after manifest is byte-identical to one baseline independently verified against all 1177 Git blobs at exact `59373066`. The set includes all tracked Rust/Cargo input files and rust-toolchain.toml. Every group's before/after HEAD is the exact final ref. Recorded environment is the BetterSSD target with jobs=2.
- Raw existing libraw build-script/C++ deprecation warnings remain in the saved logs. A strict Clippy success is not a claim of warning-free dependency build output.

The machine-readable independent audit is `/tmp/tessera-stagec-final-saved-audit-20260928.json`. This reviewer executed only file/Git/hash inspection for this native phase, never duplicated its tests or compiler work.

Acceptance: source and saved validation support integration of this bounded in-process editor/direct-setter admission and recovery-lifetime slice under the existing canonical-parent plus exact-filename key. All exclusions above remain: known case-variant physical-file alias gap, unadopted/external writers, no global Quit/crash/CAS/multi-file transaction claim, and no new Swift packaged-app or GUI acceptance from this native-only suite. Root owns final integration and any stronger exclusivity scope.
