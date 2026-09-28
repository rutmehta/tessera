# SDD ledger — plan: docs/superpowers/plans/2026-09-28-pinned-raw-descriptor.md

Base: 5f33e174. Worktree: /Users/rutmehta/.codex/worktrees/export-integration/tessera. Branch codex/pinned-raw-descriptor.

| Task/interface | Consistency review |
| --- | --- |
| Task 1 only: descriptor, tests, module export, contract | Initial whole-history validation conflicted with immutable snapshot scope; revised before execution. No inter-task interfaces. Exact bytes retained; render admission separate. |

## Coordinator rulings

- Ruling: Current-settings snapshot semantics replace the initial whole-history invariant; preserving opaque history avoids silently choosing another state. Cost if wrong: a future writable-recipe consumer needs a separate history validator.
- Ruling: Reject duplicate decoded JSON keys throughout both payloads and unknown current-settings members recursively; preserve opaque top-level metadata/history. Cost: future settings require explicit compatibility work rather than silent acceptance.
- Ruling: Exact recipe bytes participate in input identity, alongside owner/asset metadata/route/suffix; locator does not. Cost: semantically equivalent whitespace/history variants have distinct input identities. This is not pixel-cache identity.


Task 1: ready, implementation not yet dispatched. Sole compiler lane idle.

Task 1: in_progress, /root/pinned_descriptor Luna implementation/sole compiler lane. /root/hdr_nonuniform_diagnosis Astra independent edge review. No other compiler or app processes launched.

Task 1: candidate771a508f;13 focused passes/direct0 on clean committed source, four file hashes/full tracked tree hash unchanged. Root read raw log and freezes. Early draft logs preserved but exact untracked source provenance incomplete; not final gates. Independent task review has test discrimination/coverage findings and clone-on-Copy strict warning; first fix wave pending full report.

Task 1: fix round1/5 dispatched to original Luna, findings1–5 plus three cheap maintainability notes.
Ruling: Bump engine-api CONTRACT_VERSION1.6.0→1.7.0 for the additive public descriptor, following CONTRACTS.md first paragraph. Existing serialized recipes/process/goldenhash unchanged. Use user-requested Astra review in place of historical unavailable Opus. Cost if wrong: downstream clients unnecessarily treat the public API revision as new; no persisted migration is introduced.
The existing m532_channels.rs version assertion is the sole fifth-path dependency; authorize changing only1.6.0→1.7.0 there and include it in exact review/freezes.

Task 1: fix round1/5 (5 findings +3 notes addressed,0 open;771a508f..12b354fd). Root independently rehashed6809 tracked paths and all5 changed Git blobs, aggregate33d6a78cb2625b47b81151d013cabfef52345bffd642f451555207cbc2af25b8. Committed focused20/direct0; full engine-api Release/fmt/strict authorized on sole lane.

Task 1: complete (commits5f33e174..083018a9; task review clean; full98 named tests=63unit+35integration,0docs,0fail; focused20 subset; strict and targetedfmt direct0). Root read all raw final results and rehashed6809 tracked files, aggregateabbedcb6e4b022a3b4d2b173bfb3772ddf69ed7f63377b296a56822b4bc38be2. Mechanical test-only lint corrections d3a9320d/083018a9 preserved failures; no production change from reviewed12b. Fresh whole-branch Astra reports no actionable findings and ready to merge; artifact whole-branch-review.md saved. Branch pushed for provenance.

Ruling: Accept whole-branch review exclusions for physical asset verification, capture ownership, decoder/dependencies/render/pixels/cache, writable history, FFI/UI and source-backed Layers: each is a separate required gate, not established by declaration validation. Cost if wrong: a future consumer could overinterpret this API, so the public contract and board explicitly retain these boundaries.
