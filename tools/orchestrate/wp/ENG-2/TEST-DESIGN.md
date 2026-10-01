# ENG-2 test/API audit (source freeze `a187355eade82a76927c49bcd14471f42b38e25f`)

Read-only audit of the named source freeze. `HEAD` resolves to this exact commit. No applicable `AGENTS.md` exists in the Tessera checkout. The working tree contains extensive pre-existing untracked artifacts; this note does not treat them as source evidence.

## Source contract

ENG-2 requires publication of committed `Arc<DocState>` independent of the render lock, edit/commit ordering preservation, a confirm-time snapshot that excludes later edits, and nonblocking main-thread getters during an in-flight render. The requirement and later Machine B Export Flat restoration are stated in `docs/coordination/CODEX-BRIEF-2026-09-30.md`, ENG-2 paragraph (source brief, line 69 in this freeze).

`DocumentSession` owns an `Arc<Shared>`; `Shared.state` is `Mutex<State>` (`crates/tessera-ffi/src/document.rs:868–883`). `State` contains the committed document plus optional scratch/pending edit state and epoch (`document.rs:815–866`); `State::live_shared()` shares the scratch document when one exists, otherwise the committed document (`document.rs:840–846`). Render's `present_frame` takes `shared.lock()`, captures live state/view/epoch, drops the guard, then composes/presents (`crates/tessera-ffi/src/document/render.rs:992–1038`). In this freeze normal expensive compositing is outside `Shared.state`; do not conflate an intentionally held `State` mutex during edit/writer setup with a render backend busy composing. The new test should independently cover both: getter responsiveness during a barrier-held backend composite, and publication/edit order while a writer is deliberately paused with the state/writer synchronization held.

## Public reader and snapshot inventory

Exported `DocumentSession` reads in `crates/tessera-ffi/src/document.rs`:

| API | Freeze source lines | Reads / semantics |
|---|---:|---|
| `id` | 1346–1348 | Immutable id, no state lock. |
| `info` | 1350–1396 | State + history + selection bounds/cache; obtains mutable live state guard. |
| `layers`, `layer` | 1398–1417 | Flatten live layer tree and unlinked-mask state; lock-backed. `layer` delegates to `layers`. |
| `history_items` | 1926–1942 | History metadata from current document; lock-backed. |
| `snapshots` | 1968–1977 | Named snapshot list; lock-backed. |
| `history_memory_bytes` | 1995–1997 | History accounting; lock-backed. |
| `plan_surface` | 2005–2020 | Reads live canvas dimensions; lock-backed. |
| `read_level` (hidden test API) | 2403–2406 | Clones current live `Document`, then renders after lock release. |
| `document_state` (hidden test API) | 2430–2432 | Clones current live `Arc<DocState>`; lock-backed. |

`render_records`, `render_resources`, thumbnail stats/count, and `wait_idle` are renderer diagnostics/coordination rather than model getters (`document.rs:2411–2454`). `layer_thumbnail`, `mask_thumbnail`, and `composite_thumbnail` enter render helpers (`document.rs:2138–2150`); they are rendering operations and should not be conflated with cheap metadata getters. `set_selected_layers`, `set_viewport`, `set_display_headroom`, `refresh`, etc. are mutators/presentation requests, not getters. Their lock and edit ordering still need regression coverage where their behavior depends on published/live state.

Swift app paths make the main-thread importance concrete: `DocumentController` refreshes `layers`, `info`, and `snapshots` (`apps/mac/Sources/Tessera/Document/DocumentController.swift:120–146`); `EngineDocumentBackend` directly adapts `info`, `layers`, history and snapshots (`apps/mac/Sources/TesseraCore/Document/EngineDocumentBackend.swift:474–476, 560–567`). UniFFI signatures mirror the Rust exports in `apps/mac/Sources/TesseraFFI/TesseraFFI.swift`; generated C declarations are not separate behavior.

## Export Flat snapshot consumer

`begin_export_flat` validates options, locks shared state, checks open, clones `st.live().state()` into an owned snapshot and returns `DocFlatExport` (`document.rs:2214–2235`). `live()` deliberately means scratch/pending state if present, so preserve this “what the document shows” behavior; “committed `Arc<DocState>` publication” is a fast committed-state source, not permission to silently change Export Flat to ignore a visible drag. `DocFlatExport::run` consumes the captured snapshot to render/write later (`document.rs:2256–2317`). Confirm-time snapshot means later edits/commits do not affect the captured state. No pixel copy is needed because `DocState` is immutable and Arc-backed.

Current coverage is in `crates/tessera-ffi/tests/document_export_flat.rs`: CPU-composite/pixel/progress and sync parity (`:82–140`), snapshot surviving edits and close (`:189–235`), and worker run with concurrent opacity edits (`:238–282`). The snapshot test currently takes the export snapshot before *uncommitted* opacity/visibility mutations, then closes; it does not specifically prove a later committed edit is excluded. The concurrency test uses `sleep(5 ms)` and elapsed-time cutoffs, so it is unsuitable as the deterministic regression for ENG-2.

## Deterministic test design

1. **Render-in-flight getter independence.** Add the smallest test-only synchronization seam in the backend compositing path after `present_frame` has captured its `Arc<Document>` and dropped `State`'s guard (`document/render.rs`, immediately after `drop(st)` at current line 1035). A test-only pair of channels/barrier signals “snapshot captured; compositing paused” and “resume”; do not sleep or rely on a large image to make rendering slow. While backend work is blocked there, invoke representative getters (`info`, `layers`, `history_items`, `snapshots`, `plan_surface`, and hidden `document_state`) and require replies before releasing the backend. This confirms no accidental coupling to the renderer backend lock while explicitly confirming State is no longer held. Separately, pause a writer at a narrowly scoped test seam while its serialization lock is held and verify a getter is not promised to bypass a genuinely held writer/state lock unless the published-Arc design specifically makes that guarantee; record which lock contract ENG-2 intends. Use timeouts only as deadlock fail-safes. Ideally place tests under the private Rust module so the hooks remain unexported.

2. **Snapshot excludes later committed edits and retains visible scratch semantics.** In `document_export_flat.rs`, use separate deterministic cases. For committed timing, capture an oracle PNG for state S0; call `begin_export_flat` (confirm boundary); apply an observable edit, commit it, capture the S1 oracle, assert S1 differs, then run the job and assert its pixels equal S0 and differ from S1. For scratch compatibility, create a visibly different pending edit without committing, begin the export, then commit/change/close afterward; the job must render the pending live state seen at confirmation. This protects the existing `State::live()` contract while proving later commits cannot alter the captured snapshot.

3. **Edit/commit serialization and publication.** Extend existing document/session API tests (`crates/tessera-ffi/tests/document.rs`, history/commit cases around `:267–319` and source-preview/edit cases) with two clearly distinct committed states. Assert each successful commit's returned epoch/history head agrees with immediately observed `info`/`history_items`/`layers`, and that edits serialize into the same order as their commit calls. For true competing writers, use a barrier to release two operations and assert only the documented serialized order/consistent complete state; avoid assuming thread scheduling order. If the API does not promise which contender wins, assert that no mixed state appears and that resulting history nodes represent whole edits.

4. **No torn multi-field reads.** Since public getters such as `info` and `layers` are separately invoked, they cannot jointly guarantee a cross-call atomic read if a commit lands between them. Preserve each individual getter's coherent single-state read. Tests should assert per-call invariants (layer count matches rows for a quiescent barrier-delimited revision; each observed `history_head` corresponds to an existing history node), and should not assert cross-call equality while writers are intentionally running.

## Compatibility constraints and likely suites

- Committed-state reads must reflect the latest fully committed revision; an in-progress interactive edit may remain visible through the existing `live()` semantics where documented (notably `layers`, `read_level`, `document_state`, and Export Flat's current “what the document shows” doc comment).
- A confirm-time Export Flat snapshot is immutable: later edits, commit, undo/redo, close, and another export cannot mutate or invalidate the captured pixels. Preserve current output encoding/profile, cancellation, and progress behavior.
- Preserve total edit/commit order, history parentage, epoch monotonicity, saved/dirty status, undo/redo behavior, and event/listener ordering. Ensure publication never exposes a partially applied `DocOp` or stale committed pointer after a successful commit.
- Snapshot consumers outside Export Flat include save paths that capture snapshots outside `State` lock (comments at `document.rs:2156–2165` and helper implementation around `2340–2395`); keep their serialization/save ordering semantics intact.
- Primary Rust suites: `crates/tessera-ffi/tests/document.rs`, `document_export_flat.rs`, `session.rs`, and likely `develop.rs` because it exercises document-session epoch/history getters. Existing UI-facing caller coverage lives in Swift `apps/mac/Sources/Tessera/Document/DocumentSelfTest.swift` (timing/reader cases at source lines 220–235), but ENG-2 can be proven deterministically in Rust without GUI work. Inspect `crates/tessera-ffi/tests/document_adaptive_ui.rs` only if changes touch adaptive preview/read publication.
- The existing `begin_is_cheap_and_edits_continue_during_run` in export-flat tests is useful adjacent behavior but uses sleeps and 50 ms thresholds (`document_export_flat.rs:238–282`); retain it as existing perf evidence, and do not treat it as the new synchronization proof.

## Scope note

This is an API/test audit, not a proposed production lock implementation. The exact publication primitive and edit-writer lock design belongs to the ENG-2 implementation owner. No tests, builds, GUI sessions, benchmarks, checkout files, or Machine B messages were run or changed.
