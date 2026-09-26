# M5-20 implementation and verification

## Round 2 completed: caller integration

The previous out-of-allowlist blocker below is resolved under the widened
allowlist. The complete round-2 gate passes; historical round-1 failures are
retained below for context.

- MCP schema generation now mirrors the channel and people value types and all
  five LibraryToolCalls with their real serde envelopes/defaults.
- All five channel document calls dispatch to compositor operations and
  selection::channels::load. DocumentSummary exposes ordered channels. A
  session-wide ID floor prevents reuse across undo branches, including save_as.
- Host-owned content-addressed raster staging is available both in Rust and via
  stage_channel_raster. The README documents sample/shape validation, bounded
  retention, replay lifetime, and a selection-to-channel workflow with no inline
  pixel buffers. Channel Actions bind newly created IDs symbolically.
- All five people calls dispatch to index/cull APIs through MCP, Console and
  ActionCall::Library. Persistent numeric ID adaptation covers existing opaque
  ml_faces identities, retiring deleted bindings rather than resurrecting them.
  tessera://people exposes the bindings and summaries. Defaults do no sidecar I/O;
  opt-ins support MWG export and additive catalog/XMP keywords.
- Library metadata writes are not crash-atomic with membership mutations. Errors
  state when catalog effects already applied. Per-face assignment/confirmation
  transaction limits, serialized-host requirements, approximation provenance
  limits and ID-map retention are documented in crates/tessera-mcp/README.md.
- No FFI/CLI source changes were necessary. Round-1 engine-api contracts and
  legacy compatibility fixtures remain intact. Only the cull dependency was
  added to MCP and Cargo.lock.

Verification executed after review corrections:

    cargo test -p engine-api -p tessera-mcp --release && cargo clippy -p engine-api -p tessera-mcp --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace

Exit 0. 135 tests passed, zero failures, zero ignored. Strict Clippy, workspace
formatting and workspace checking passed. Existing vendored LibRaw C++ warnings
remain. Full output: round2-validation.log. Cargo commands retained
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-20.

Tests cover channel add/summary/load/edit/rename/delete/undo, malformed raster
references and samples, channel ID non-reuse, channel Action rebinding, actual
MCP staging and spot-channel dispatch, all people mutations, invalid references,
name clear, default write opt-outs, explicit MWG and keyword opt-ins, opaque ID
mapping across reopen and post-merge key recreation. Regression tests were
observed failing before their fixes (review-*-red.log and staging-red.log).
A read-only review identified channel undo-ID reuse and people key resurrection;
both now have passing regressions. This was not an Opus review.

git diff --check passed. Changed/untracked paths were checked against the widened
allowlist. No commits and no repository target/ files.

RESULT: PASS

## Delivered within allowlist

- Contract version 1.3.0; recipe schema 3.
- Numeric ChannelId with alpha SelectionId numeric mapping; ChannelKind (alpha/spot display RGB + solidity), ordered ChannelSummary records on DocumentSummary independent of active selection.
- Add/delete/rename/edit/load-channel-as-selection document calls, existing history/concurrency envelope, name registry and action conversion. ChannelRasterRef addresses host-staged immutable single-channel content without inline pixel buffers. DocumentEdited returns the allocated/affected channel ID, including spots.
- Existing numeric PersonId preserved. Composite FaceRef, PersonSummary, normalized center/size FaceRegion with coordinate-space dimensions, optional-name cosine NameSuggestion, QualityGate, ClusterOptions, PeopleJobResult and explicit PeopleWriteOptions.
- Separate LibraryToolCall/LibraryToolRequest for assign, confirm/unconfirm, merge, split and name/clear. Both write opt-ins default false. Library actions are catalog effects, not recipe/document history edits.
- Typed Recipe.source_kind. Legacy absence means Raw; the formerly flattened top-level source_kind string is consumed into the field and saved lowercase. Raw/Rgb aliases also normalize. Unrelated unknown members survive. Raw golden hashes remain unchanged; RGB hashes and stage seeds distinguish the source route.
- 1.2 compatibility JSON and 1.3 typed schema fixtures, round trips, defaults, action registry/envelope tests, invalid source-kind tests, and source-route cache separation tests.
- CONTRACTS.md updated with semantics, defaults, compatibility limits, host responsibilities and 1.3 changelog.

## Verification actually run

CARGO_TARGET_DIR remained /Volumes/betterSSD/tessera-cache/target/M5-20 throughout. No builds into repository target/.

Exact requested gate executed twice, most recently after review correction:

    cargo test -p engine-api --release && cargo clippy -p engine-api --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace

Final exit: 101. Full output: verification.log beside this report.

- engine-api release tests: 74 passed, zero failed (63 unit + 11 integration); zero doc tests.
- engine-api Clippy all-targets with -D warnings: passed.
- Workspace cargo fmt --check: passed.
- cargo check --workspace: FAILED in tessera-mcp, with 14 compile errors from the newly extended contracts.
- git diff --check: passed. Changed and untracked paths checked programmatically against the two allowed prefixes; no out-of-scope files.
- No commits made and no callers or bindings changed.

New functionality was tested before implementation. Source-kind and channel tests first failed at runtime on the old behavior; people/library tests first failed on missing API symbols. Each targeted suite then passed. A read-only secondary review found that allocated spot IDs could not be returned in DocumentEdited; added a failing regression, the optional channel response field, and reran the complete gate. This was not an Opus review.

## Blocking caller integration (not modified)

The task simultaneously requires new public enum variants/summary fields, forbids changing callers, and requires a workspace build. JSON backward compatibility cannot preserve exhaustive Rust matches or full struct literals. The gate therefore cannot pass with the current callers under this allowlist:

- tessera-mcp generated wire_schemas.rs: missing ChannelKind, ChannelRasterRef and ChannelId schema definitions/aliases (build.rs needs integration).
- crates/tessera-mcp/src/documents/mod.rs:382: DocumentEdited initializer lacks channel.
- crates/tessera-mcp/src/documents/mod.rs:402: exhaustive DocumentToolCall match lacks the five channel operations.
- crates/tessera-mcp/src/documents/mod.rs:1140: DocumentSummary initializer lacks channels.
- crates/tessera-mcp/src/actions.rs:486: exhaustive ActionCall match lacks Library.

These are follow-up integration work, not silently bypassed with feature flags or placeholder dispatch implementations. Runtime validation, raster staging/retention, ID adaptation to crate-local people IDs, library mutation execution and source routing remain host responsibilities.

## Missing source note

The requested tools/orchestrate/wp/M2-29/REPORT.md does not exist in this worktree. The available brief/prompt/attempt log mention persisted source_kind but do not contain report item 4. Implemented the explicit inference/alias policy in the M5-20 user brief and documented that policy in CONTRACTS.md.

## Retry confirmation

Re-executed the exact four-command gate with the required external
`CARGO_TARGET_DIR`. See `retry-verification.log`. Release tests, strict
engine-api Clippy and workspace formatting passed again. Workspace checking
still exits 101 with the same 14 tessera-mcp errors listed above. No source
changes were made during this retry: removing the required variants/fields,
hiding them behind disabled features, or modifying callers outside the allowlist
would not satisfy the requested contract and scope together.

Unblocking requires permission for a separate tessera-mcp integration change
(schema generator, document dispatch/summary construction and action dispatch),
or deferring the workspace-check gate until that follow-up is integrated.

RESULT: FAIL cargo check --workspace requires tessera-mcp caller/schema changes outside the allowed paths.

## Latest verification

Ran the exact requested gate again with the external target directory unchanged.
The refreshed `retry-verification.log` records exit 101: every engine-api release
test passed, strict Clippy passed, and workspace formatting passed. Workspace
checking reproduced all 14 tessera-mcp errors above. Inspected the MCP schema
generator: its explicit type allowlist excludes the new channel types. The
remaining errors are exhaustive matches and struct literals, which serde defaults
cannot repair. No source changes or out-of-scope edits were made in this retry.
The task remains blocked on permission to integrate those callers or defer the
workspace gate, rather than weakening the requested public contracts.
