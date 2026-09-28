# Editable Smart Preview status

## Current implementation — 2026-09-28 19:04 UTC

Smart Preview generation, explicit proxy editing, offline Library, local edit/save/reopen, reconnect synchronization and local proxy thumbnails are integrated on main `89b78881`. B supplied Swift controls/routing/retry; A integrated and qualified the native/desktop bridge. Full Swift:689 XCTest cases,1 skipped,0 failures plus5 Swift Testing cases; strict optimized product and actual Sony offline workflows pass. Native thumbnail:187 FFI tests and28 previews tests pass (3 existing ignored). Evidence `62e4ee5b`, `cae2004d` records source/artifact hashes and retained failures.

Current Compact proxy is1640×1092 and7,319,246 bytes for the16,646,144-byte Sony RAW (56.03% smaller); it retains camera-linear editing data rather than a finished JPEG. Legacy Detail/v1 assets remain readable. Edits are stored as a recipe and full-quality export uses the original; proxy/source ownership and conflict guards remain enforced.

Original remains the default. CPU proxy rendering has not established a speed advantage over the existing Original Metal renderer. The opt-in GPU candidate is applied only in the feature checkout for qualification: its first actual Metal suite passed8 and failed2 numerical-parity checks. It is unaccepted and not merged. No accelerated, physical input-to-display, cross-camera or packaged-app GUI claim is made. Actual local-profile GUI build/offline edit/separate-process restart/reconnect sync and Original-setting persistence passed; evidence `6c38697e`. GUI full4920×3276 export, offline full-quality export refusal and two-proxy Compare also passed; root verified output hashes/dimensions and viewed the decoded export. External-volume profile startup and performance acceptance remain open.

## Historical implementation checkpoints
## Current implementation — 2026-09-28 17:04 UTC

Camera-linear proxy generation, bounded persistence, original-identity journal, native offline Develop save/reopen and conservative synchronization are integrated. Final native candidate e1eca7ba passed171 unit tests plus real Sony build/edit/clean-and-dirty offline restart/reconnect/conflict/full-resolution export workflow. Original photos stay unchanged during proxy generation/local editing. Source/evidence are published on main ff0459fc.

Full desktop feature remains in progress: cached offline Library, current Swift controls/bindings and actual app workflow gates remain. The current Detail proxy is only0.084% smaller than this Sony compressed RAW; Compact source preparation is unqualified. CPU proxy warm edit384.7ms beats originalCPU818.2ms but loses to originalMetal33.07ms in matched-output tests. Original remains default; explicit/offline proxy use is supported by the native API. GPU acceleration is being prepared, not accepted. Earlier inventory below records the preimplementation state.


Status: NOT IMPLEMENTED in the current source inventory, 2026-09-28. This is a status audit, not a completed feasibility design or acceptance gate.

The user asked about lightweight versions of RAW files that remain editable. Existing project design docs/05-catalog-storage-and-import.md section2.2 and docs/01-lightroom-classic-spec.md section1.11 describe approximately2560-pixel lossy-DNG Smart Previews for offline editing, with originals used for full-quality export when available. Those are intended behavior, not evidence of implementation.

Existing implemented building blocks:

- crates/previews/src/lib.rs implements content-addressed JPEG preview pyramids; keys include file hash, orientation and recipe hash. These are rendered display caches.
- crates/tessera-ffi/src/develop.rs::open_develop_session obtains the indexed path and calls RawImage::open on it. It does not select an editable proxy when the original is offline.
- Develop uses reduced rendering levels and caches for interaction; these do not constitute a separately persisted, independently editable RAW proxy.
- crates/export/README.md explicitly states its DNG slice exposes no lossy-DNG option. Linear-DNG reader/writer support must not be relabeled Smart Preview generation.
- Pure pinned descriptor source083018a9/mainbed7ff3f preserves declared source identity and exact recipe snapshot. It neither creates a smaller source nor adds offline editing.

Searches across crates/apps/docs and fetched Git history found Smart Preview specifications but no implemented proxy generator, durable proxy/source link, editor proxy fallback or source-status UI. No branch/commit matching Smart Preview/lossy-DNG/offline-editing implementation was found. This inventory cannot prove no unpublished code exists on an inaccessible machine.

Before implementation, define a proxy representation that retains an honest supported Develop contract; avoid reapplying RAW operations to an already fully developed JPEG. Required gates include build/discard lifecycle and storage budget, original/proxy identity and stale detection, recipe ownership, original-missing editing, original reconnect, export resolution/quality behavior, and visible Original/Smart Preview/Unavailable state. Full-fidelity equivalence for every RAW-only/AI/detail control is not assumed.

Follow-on Astra feasibility audit and Luna private-capture slice audit were dispatched but both terminated with a Codex usage-limit error; neither produced its requested report. They are failed/incomplete, not running. No new proxy implementation, decoder, benchmark or UI run occurred. The completed descriptor acceptance remains valid and separately documented.

## Coordinator source follow-up

Source-only inspection after the follow-on agent limit found a concrete reuse constraint:

- `crates/export/src/lib.rs` DNG branch calls `render_full_float(image, recipe)` or the AI-mask render path before writing. `crates/export/src/dng.rs` explicitly describes finalized **developed** float32 LinearRaw DNGs. This is not a demonstrated pre-edit proxy-generation boundary.
- `crates/image-core/src/source.rs::RawImage::open` recognizes LinearRaw DNG and returns `RgbSource::from_linear_dng`. `crates/tessera-ffi/src/develop.rs::open_develop_session` then assigns recipe.source_kind from the decoded route. Therefore merely redirecting an original RAW's editor path to the existing developed-DNG export would select RGB semantics and risk reapplying already baked edits. This is a source-grounded risk, not an executed failure experiment.
- The accepted pinned descriptor freezes an immutable recipe snapshot for future Layers use. An editable proxy instead needs the original asset/recipe owner to remain authoritative while current edits evolve, plus an independent proxy-generation identity and original/proxy render-route distinction. Do not reuse immutable-snapshot semantics as the whole Smart Preview contract.

Next design gate: define a pre-edit proxy-generation boundary and supported control matrix, then tiny original-versus-proxy tests before storage/UI work. Cover white balance/calibration, demosaic, geometry coordinate scaling, masks and AI/detail controls explicitly; refuse or require the original for unsupported operations rather than claiming universal equivalence. Source selection must not silently overwrite the original recipe's source/process identity. A full-quality export must resolve the original or give an explicit unavailable/limited-resolution outcome. The existing intended2560px lossy-DNG format is not proven by the developed-DNG writer.

No implementation, codec experiment or proxy fidelity test was run. Independent feasibility review remains blocked by the observed agent usage limit.
