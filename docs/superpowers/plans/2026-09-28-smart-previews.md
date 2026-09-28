# Editable Smart Previews Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development task-by-task. User authorizes autonomous execution and parallel independent workers; A serializes compiler/GPU workloads and owns merges.

**Goal:** Edit reduced camera-linear RAW proxies quickly, save edits offline, and apply them to matching originals for full-quality output.

**Architecture:** Extract the existing RAW prefix into an explicit camera-linear source, retain original calibration and resolved prefix, and route reduced renders through the existing CPU tail. Store proxy assets and an independent durable recipe journal locally, with shared edit-owner admission and conservative reconnect.

**Tech Stack:** Rust pipeline-cpu/image-core/tessera-ffi, local atomic filesystem persistence, Swift macOS Library/Develop.

**Spec:** docs/superpowers/specs/2026-09-28-smart-previews.md

## Global constraints

- Maximum proxy long edge 2560; source photos immutable.
- Native revision 2 initially; explicit unsupported/source-required errors for incompatible prefixes or sensor operations.
- Original ImageId remains recipe owner; separate render/payload identity.
- No full-quality proxy export or silent recipe rewrites.
- B Document/Save As sources reserved; no overlapping edits.
- Heavy tests serialize on A; no protected UI bypass.

## Review focus

Nonneutral WB must detect the working-RGB trap; default lens corrections must not run twice; odd crops/orientation must preserve normalized coordinates; stale original or journal cannot overwrite newer edits; offline success must survive reopen with no original volume.

## Task 1 — camera-linear CPU source (accepted component)

Files: crates/pipeline-cpu/src/render.rs, new camera-linear source module as needed, src/lib.rs and focused integration tests.

- [x] Extract pre-matrix CFA prefix into an owned camera-linear value with original calibration, active-area mapping, settings fingerprint and resolved lens operations.
- [x] Add explicit render source route. Preserve camera profile then WB then current downstream order. Record applied CA/embedded stages; reject incompatible prefix or unsupported late coordinate transforms before rendering.
- [x] Add tests: custom/as-shot WB with nonidentity calibration, scale-one CFA comparison, editable exposure, upstream mismatch, HDR finite values, odd crop/reduction, default Auto/CA.
- [x] Run focused Release tests with source hashes/direct exits, retain failed attempts, commit exact component and obtain independent review.

## Task 2 — durable local recipe journal (accepted component)

File: new crates/tessera-ffi/src/smart_preview_store.rs; integrate module only after Task 1 compiler lane release.

- [x] Define versioned records keyed by original ImageId, exact recipe bytes, original content identity, captured sidecar baseline, monotonic journal revision and dirty state.
- [x] Atomic create/update and reopen with bounded lengths, digest validation and revision preconditions. No original-path writes. Refuse dirty discard.
- [x] Test stale revision, wrong owner/version/corruption/oversize, unknown-member preservation, crash-safe published record and edit/reopen.
- [x] Compile and run focused tests in A's serialized lane; independent review before caller integration.

## Task 3 — persistent proxy codec and image-core routing

Files: new bounded codec/store module near image-core or previews, image-core/src/source.rs, render.rs, rgb_render.rs and resident routing guards; Cargo only when codec needs it.

- [x] Persist the qualified Task 1 source, including validated camera/prefix data and exact source/payload identities; use a measured compressed representation without clipping HDR or negative values.
- [x] Add explicit camera-linear accessor/source-kind and source-discriminated memo keys. Route all CPU/M2 entry points consistently; deny unsupported resident/export routes.
- [x] Round-trip pixels/calibration/prefix; reject corrupt/oversize input before allocation; test original/proxy cache-switch isolation.

## Task 4 — owner admission, build/open/save/reconnect

Files: tessera-ffi/src/recipe_write.rs, develop.rs and dedicated smart_preview service; existing Engine setter call sites.

- [x] Add stable original-ImageId admission shared by normal and proxy editors/direct setters, while retaining destination collisions and online baseline checks.
- [x] Build from coherent original bytes without changing sidecars; publish asset+clean journal only after verified completion. Failed builds leave no advertised partial preview. Build is synchronous; UI cancellation stops between photos, not within a build.
- [x] Open proxy with original recipe identity, save local journal durably, reopen offline without original-parent canonicalization.
- [x] Reconcile matching original content and sidecar baseline; keep conflict copies and report conflict otherwise. Original export must require successful reconciliation and matching source.
- [x] Test normal/proxy lease races, failing-open release, offline save/restart, original mutation and external sidecar edits, clean/dirty discard, export source choice.

Native acceptance: e1eca7ba, merged on main before ff0459fc. Final171 FFI unit tests and real Sony clean/dirty offline workflow passed; strict/fmt passed. This does not include app offline Library reopening, which needs the separate catalog-backed session API.

## Task 5 — Library and Develop controls

Files: Swift EngineLibrary/reference adapters, AppModel Develop opener, AppCommands Library menu and ThumbnailCell status; generated FFI bindings only from accepted native API.

- [x] Batch Build/Discard actions, cancellation/per-item results and explicit proxy-use preference/source selection.
- [x] Show original/proxy/offline/stale/conflict state with accessibility labels; generation-safe completion cannot replace newer selection/status.
- [x] UI/model tests for transitions, source choice and failed export. Actual supported app workflow with Sony ARW: build, edit, offline restart/save, reconnect/export; retain original hash.

UI/model gates and actual Swift/native Sony offline workflow pass on `c1f9d4e0`, merged `89b78881`. Actual packaged-app local-profile Sony build/offline edit/process restart/sync/full original export/offline export refusal/Compare passed (GUI-COMPLETION.md). Interactive performance remains open.

## Completion gate

- [ ] Independent component and whole-branch reviews, relevant strict tests, codec size/fidelity and interactive performance measurements.
- [ ] Preserve all evidence and failures; update board with exact commit/outcomes. Merge only accepted components; do not call the feature complete before end-to-end gates.
