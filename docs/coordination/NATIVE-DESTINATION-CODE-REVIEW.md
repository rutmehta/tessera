# Native Save As destination intent review

Reviewed candidate `bcbf6ddefd80539c03890c9d4965b26ef1669373` against `f093e3c872b3b00eb1190e941dc987daaaf1ba4c`, including the accepted decision `docs/coordination/SAVE-DESTINATION-INTENT-DECISION.md` and design `tools/orchestrate/wp/B5-16/SAVE-DESTINATION-INTENT-DESIGN.md`. Read-only review only: no checkout edits, compilation, tests, or runtime activity.

## Verdict

I found no source-level blocker in the bounded native implementation. The new checked API is additive: `CreateIfAbsent` maps to a no-clobber staged publication and `ReplaceConfirmed` maps to the existing replacing publication. Legacy `save_as` still chooses Replace, and ordinary `save()` uses the same replacing helper. I found no identity-CAS requirement or accidental removal of explicit replacement behavior.

## Source findings

- `crates/tessera-ffi/src/document.rs:2110-2120` takes `Shared::saving` before reading the current path for ordinary Save and holds it through snapshot/publication. Checked and legacy Save As take the same gate (`:2126-2144`, `:2177-2186`). This avoids a concurrent Save As causing ordinary Save to write a stale path.
- `:2187-2209` updates title, path, and the engine path map only after `Saved`; `DestinationExists` returns before those changes. `save_snapshot_with_mode` (`:2219-2239`) updates `saved_node` only after publication reports `Saved`. A collision leaves the previously saved markers alone. The pending edit may be committed into history before an attempted save, as the decision doc correctly disclaims rollback.
- `crates/tessera-ffi/src/document/io.rs:345-388` stages in the destination directory, writes and syncs the staged file, then uses `persist` for Replace or `persist_noclobber` for CreateIfAbsent. Only `ErrorKind::AlreadyExists` becomes the typed collision (`:366-378`); other I/O errors remain errors. No pre-check-then-rename or destructive destination cleanup is present.
- `:396-419` routes native, PSD, and PSB document-session saves through the same staged commit helper. Native output reuses `compositor::format::to_bytes`; PSD/PSB retain their serializer and size/version checks. The unrelated PSD-copy transaction is left unchanged.
- `persist_noclobber` cleanup behavior is accurately qualified in the source comments (`io.rs:366-372`) and in the accepted decision: the tempfile fallback may publish successfully while failing to make staging-name removal observable. This is not misreported as a failed/untouched save. The implementation does not promise complete stage cleanup or power-loss durability.
- Post-publication session lock/close errors remain a documented existing boundary: once publication succeeds, later marker/path update errors can leave file and in-memory metadata out of sync. The candidate does not re-label these as destination conflicts. This residual is explicitly disclosed in the design and is not a blocker for this bounded slice.

## Test evidence quality and remaining limits

`crates/tessera-ffi/tests/document_save_destination.rs` exercises the real `DocumentSession` API. Collision tests verify sentinel bytes and path/title/dirty/history markers remain unchanged; fresh native/PSD/PSB outputs are reopened through a new `Engine` and decoded to a 4×4 document (`:22-31`, `:34-80`, `:82-126`, `:194-216`). The tests also verify that confirmed replacement overwrites a post-approval external edit, and legacy `save_as` plus ordinary `save()` still replace an existing native destination (`:218-232`). Existing `tests/document.rs` continues to cover PSD layers/composite and PSB decoding after legacy saves.

The no-clobber helper tests (`document/io.rs:897-1011`) cover a file arriving after staging, distinct stage names, the atomic helper race, dangling-symlink preservation, existing-file preservation, and Replace. The two-prestaged-helper race uses a barrier immediately before commit, so it does exercise the actual atomic commit boundary. The separate real-session race (`document_save_destination.rs:128-191`) synchronizes just before the two API calls, not after both sessions have staged output. It can therefore serialize in scheduling; it still verifies one typed winner/loser outcome, marker behavior, and that the winning file reopens. This is a useful integration race test, but not a deterministic session-level staged-race test.

Two small coverage limitations remain, neither a source blocker:

1. The directory destination control accepts either a typed `DestinationExists` or a generic I/O error (`io.rs:992-997`). It proves the directory is preserved, but does not pin the intended typed classification for a directory on the target filesystem. The accepted decision asks for actual errno classification on file, symlink, and directory; this case should be reported as unverified until that target-platform result is captured.
2. The native/PSD/PSB reopen helper checks decoded extent, not pixel/layer equality. The collision/race output is genuinely opened by the engine decoder; richer PSD layer/composite round trips remain covered by existing `tests/document.rs` rather than this new focused file.

The added legacy replacement test is native-only, but the common `CommitMode::Replace` mapping is directly shared; existing document tests continue to save/reopen PSD and PSB with layer/composite assertions. No regression to legacy Save/Replace is evident from source.

Status: source review accepted for the native slice, with the two test-boundary limits above and cleanup observability explicitly retained. No tests or runtime acceptance are claimed by this review.
