# Next A-owned engine task reconciliation

Read-only review against main `d090bbf7242635d588b29e527240b432550b83d2`. No builds, GPU, source edits, branch changes, or B ownership transfer.

## Recommendation

Resume the already authorized **bounded private RAW captured-stream ownership primitive** as an engine-only source plan, then implementation after review. This is the next ready engine implementation prerequisite, **not a merge-ready product candidate**. If “integration/repair” strictly requires a tested existing candidate, no new ready candidate was established beyond the excluded Save As and current Smart Preview work; do not invent one.

Exact existing prerequisite branch: `codex/pinned-raw-descriptor` at `083018a967d2ba1b07570824e885baca1e9d4314`, already ancestor of main: True. Integrated by `bed7ff3f` with full engine-api98 tests, strict/fmt and evidence under tools/orchestrate/wp/UX-05/evidence/2026-09-28/pinned-raw-descriptor-083018a9/. Descriptor implementation is crates/engine-api/src/pinned_raw.rs. It validates declared inputs only; no capture owner/storage primitive exists there. Source scan found descriptor and synthetic CPU profile tests, not the proposed sealed-capture implementation.

The historical TASK-BOARD/MACHINE-A notes explicitly record the next private-capture planning agent failing with usage-limit error, no report produced, not running. The later Smart Preview task superseded its sibling feasibility investigation, but did not implement the general pinned-input capture contract. Do not restart the completed descriptor work or confuse Smart Preview's private build snapshot with a verified reusable pinned-source API.

Smallest concrete next authorized action: write the missing engine-only capture implementation plan from LIVE-RAW-CAPTURE-POLICY.md and LIVE-RAW-PINNED-SOURCE-PROPOSAL.md, choosing a narrow owned-stage API, byte/temporary-storage bound, cancellation/failure cleanup and decoder-handoff ownership. This can occur source-only while FFI retains runtime. No B Document/Swift adapter edits, no new graph node, no GUI or render-admission claim. Existing proposal mandates hashing the exact staged stream and decoder reads solely through the capture owner; do not claim point-in-time atomicity against external in-place writers. The source plan must explicitly distinguish held capture-owner lifetime from fully owned decoded output.

Remaining acceptance for the primitive: deterministic byte/domain-digest parity, bounded read/size/quota/cancellation failures with no publication, original replacement after freeze cannot redirect the injected consumer, captured artifact digest mismatch rejects before consumer invocation, delayed consumer retains owner until completion then cleanup, honest mixed-stream control. After implementation review, focused/full affected crate tests, strict/fmt and frozen input evidence. Actual decoder/ICC/environment identity, render-profile admission and future B adapter remain separately unimplemented; these must not block the bytes/ownership primitive or be falsely claimed delivered by it.

## Rejected stale/blocked alternatives

- `wp/M2-45d` exacthead`69bcd3e5d3d30ac334720ec6a02e1688b6f6b5cb` is already ancestor of main: True. Integration occurred through `5e5593c0` (“Merge validated DNG HDR and native metadata export slice”). Historical “integration still required” note is stale. No repeat merge/gates justified. ISO gain-map interoperability remains a separate open acceptance; Phase10 is already investigated, and the proposed boundary-shift control was rejected as confounded.
- `wp/M5-31` exacthead`97eb4cac2e4722e4b1cea310220040110cb2e997`, product`441da3e3`, is ancestor of main: False. Not merge-ready: preserved MachineB original cold106.084084ms exceeds100ms; prior diagnostics neither reproduced nor explained it. Repeating the same investigation/threshold relaxation is not a ready action.
- Destination alias exclusivity is a genuine limitation, but current reviewed decision explicitly retains exact-key scope and defers persistent sentinel side-effect/cross-process contract. Not an authorized silent implementation now.
- Smart-filter mask thumbnail retention is B-reserved DocumentFFI; no ownership intrusion.
- M2-58 actual presentation remains a host/UI acceptance issue, not a fresh non-UX engine task.
- RES01/02/03/04, PSD preflight, saved-recipe histogram and StageC admission are recorded integrated; do not reopen completed bounded slices merely because broader claims remain excluded.

Disposition: one actionable ready **source-preparation** task identified, no additional merge-ready tested engine branch. Root may assign the bounded capture plan now without consuming compiler/runtime lane, then approve its concrete implementation scope.
