# UX05 Document / FFI / persistence review (proposal only)

Request0dbfc6de-f772-438e-a164-edfba18f823b, reviewed main0f436436,
2026-09-28 UTC. Exact B target/expiry validated; accepted receipt before review.
No graph implementation, product edits or workloads. This note does not approve
or advertise a live feature. Existing Open in Layers remains a rendered copy.

## Current facts confirmed

The draft accurately traces DocumentWorkspace.editInLayers to
open_document_from_image. io::open_image renders through export::render_pixels
with Resize::None and sRGB profile, creates one U16 pixel layer and records
source_image_id only in Opened/session state. Source ID is not in native MDoc.
Session reuse key is image:{image_id}:{developed}; it intentionally reuses the
existing copy. A future linked path must use a distinct key and API rather than
silently changing this method or reusing the flattened session.

LayerKind and serialized MKind have no RAW source node. Format version is1,
magic TSRDOC\\0\\x01, native load creates Document::new from saved state and
starts a new history. Recipe render hash excludes history, IDs, selection and
unknown fields; it includes source kind, process version and settings.
catalog::document checks recipe sidecar image_id against the requested ID, but
its XMP/default fallback assigns the supplied ID. Therefore that helper alone
cannot establish file identity/content for a live resolver.

## Corrections / missing decisions before implementation

1. Saved recipe hash/process is not a retrievable historical recipe. The draft
   simultaneously follows the authoritative latest recipe and expects saved
   render reproducibility. Choose a policy. Proposal: first internal slice pins
   an immutable recipe snapshot; later FollowCommitted mode resolves committed
   revisions explicitly. Store enough render settings to reproduce a pinned
   hash, or require an immutable engine revision store. A hash alone is not enough.
   The pinned snapshot is immutable render input, not a second writable recipe.
2. ImageId identifies a master OR virtual copy (engine-api/id.rs:91). Separate
   recipe identity from physical asset identity. Copied sidecars/duplicate IDs,
   rebuilt index, changed bytes at same path and missing sidecar require explicit
   outcomes. Path and mtime are hints, not content identity. No blind fallback
   through catalog::document assigning a requested ID to arbitrary located bytes.
3. Recipe hash is content identity, not a monotonic notification revision. Undo
   can return to an earlier hash. Track an independent subscription/request
   generation for stale completion; do not use document epoch or recipe history
   node alone as a cross-process ordering clock. Define whether notification
   follows committed sidecar persistence or transient Develop previews. Recommend
   committed updates only initially; no observing half-written recipe state.
4. Native document undo history is not persisted. Wording about preserving both
   histories across reopen must mean source recipe history plus saved document
   STATE, unless history persistence is separately designed. External refresh
   must not masquerade as a document user edit or enter source undo implicitly.
5. FORMAT_VERSION2 alone is insufficient for a clean old-reader failure: current
   from_bytes deserializes Manifest/MKind before checking version, and unknown
   variants fail parsing first. New reader should preflight format/version from
   a minimal envelope, accept v1, then parse v2. Existing old reader may give a
   generic parse error; it cannot be retroactively made to preserve unknown nodes.
   Safest initial policy is fail closed, never flatten/drop/overwrite unsupported
   nodes. Preserve original file; do not promise unknown graph round-tripping.
6. Color/geometry is missing from source contract. Existing copy is oriented,
   full-resolution, U16 sRGB with baked Develop settings. A linked renderer must
   declare orientation, crop/canvas, sample depth, alpha and color profile. Do not
   silently equate that result with scene-linear RAW or apply Develop twice.
   Geometry-changing recipes need explicit resize/rebase policy; initially reject
   unsupported dimension/profile changes rather than moving masks silently.
7. Dirty state currently derives from document history/saved node. Merely bumping
   a frame epoch won't make a newly accepted external recipe serializable/dirty.
   Recommended first policy: pinned save remains stable; external committed
   update reports AvailableChanged until explicit refresh. Refresh replaces the
   pinned descriptor as a document edit and marks document dirty; source recipe
   is unchanged. This is an internal staged capability, NOT automatic live follow.
   Automatic follow requires a separate accepted dirty/undo/conflict policy.
8. Reading/saving a source descriptor must not resolve/render as a side effect.
   Current native format writer has a fixed .tessera-doc.tmp path and no resource
   admission; don't claim multi-writer transactional or crash-durable persistence
   from it. Resolver needs independent cancellation and bounded render admission,
   with no engine/catalog/session lock held during decode/render/callbacks.

## Minimum versioned model and resolver contract for joint review

Proposed LayerKind::RawSource(SourceRefV1) and native MKind::RawSource with
source_schema_version:1, within native manifest v2. Common LayerProps/masks remain
existing document state. Avoid two competing placement transforms: first leaf
renders at fixed local extent; transformed placement can use an existing
SmartObject container after A audits that recursive path. No new graph edit is
being implemented or approved by this proposal.

SourceRefV1 fields:
- binding_id (document-local stable identity, distinct from layer ID);
- recipe_image_id and asset identity/content_digest with named digest scheme;
- portable relative locator hint plus optional platform bookmark/absolute hint,
  never authoritative; no mandatory catalog-root dependency;
- recipe schema/process/source_kind and immutable render snapshot + recipe_hash
  (or agreed resolvable immutable revision), last accepted source digest;
- declared extent/orientation/profile digest/sample contract, render contract
  version, update_policy:Pinned initially. Reserve but do not enable FollowCommitted.

Do not persist runtime epoch, subscriptions, pointers or decoded source handles.
Last-known preview pixels are optional cache with provenance, never a silent
replacement for a missing source. Initial slice can omit offline pixels entirely.
Do not assume unknown top-level recipe data affects today's recipe_hash; reject
unsupported future schema rather than render with silently discarded semantics.

Proposed engine-owned internal interfaces, not UniFFI commitments:

resolve(SourceRefV1, cancellation) -> ResolvedSource | ResolveIssue
ResolvedSource = immutable source lease + exact content digest + immutable recipe
snapshot/hash/schema/process + geometry/color contract + resolver generation.
ResolveIssue = MissingAsset, IdentityMismatch, ContentChanged, RecipeChanged,
UnsupportedSchemaOrProcess, UnsupportedGeometry, AccessDenied, Cancelled, Failure.
RecipeChanged/ContentChanged carry a candidate description, never silently accept.

render(ResolvedSource, level/region, target contract, cancellation) -> tiles plus
exact cache key. Key includes asset digest, recipe render hash/process/source
kind, render contract, profile, extent/orientation and level/region. Keep upstream
source key separate from downstream layer transform/mask/blend dependencies to
avoid unnecessary redecodes. No promise of native ROI decode if engine renders a
whole source; admission must account for actual intermediate allocations.

subscribe(recipe_image_id, last observation) -> generation-tagged committed-change
notification; unsubscribe on close/relink. Notification only schedules resolve;
publication validates binding identity + requested generation + content/recipe
key. Old completion cannot replace newer state, even when hashes cycle via undo.

Minimum B-facing FFI after A stabilizes model: separate open_linked_raw_document
or insert_raw_source entry (name reviewed before export), inspect source status,
explicit refresh/relink operation with expected binding generation, and listener
status/update event. Use request-owned cancellation independent of backend lock.
Existing open_document_from_image/developed flag, rendered-copy disclosures and
save_psd_rasterizing_transforms contracts remain unchanged. Generic PSD export
must explicitly reject unresolved/live source kinds until a separately tested
rasterized-copy route exists; never serialize them as empty pixels.

## Ownership and sequencing

- A: engine-api identity/revision types, read-only authoritative resolver and
  recipe writer coordination, source leases, decoding/render/color contract,
  cancellation/admission, committed-change events; compositor LayerKind/cache
  integration and native format migration/reader/export match coverage. Main merges.
- B: DocumentSession/FFI adapter, per-binding request ownership/status publication,
  Swift Document model/UI disclosure and explicit refresh/relink controls,
  tests asserting native round-trip behavior through FFI. B reviews persistence
  contract but does not independently edit A's compositor/format implementation.
- Joint checkpoint before product work: pinned versus follow policy, locator and
  virtual-copy identity, color/geometry, dirty/undo, missing/future-version behavior.
  Keep product copy path intact throughout. A's recipe-writer guard remains its
  own queue; this proposal is not authorization to bypass it.

## First narrow acceptance scenario

First gate is INTERNAL persistence/resolver vertical slice, not a shipped live UI:
use one known tiny RAW fixture and pinned recipe A in a separate new document,
fixed geometry/profile, one source node plus non-default layer opacity. Save
v2, destroy session/engine, reopen against rebuilt index but retained source and
sidecar, resolve exact asset + recipe A, compare rendered pixels within a stated
engine tolerance. Assert descriptor remains source-backed, opacity survives,
RAW and recipe bytes unchanged, source recipe history unchanged; document history
may reset per existing native contract. Existing v1 opens unchanged.

Negative variants in same small gate: missing asset, same-path replacement,
wrong-ID sidecar, unsupported source schema; preserve graph and original file,
report explicit unresolved/failure, never render unrelated pixels. Change recipe
A to B externally: reopened pinned document must not silently adopt B; report
AvailableChanged and keep pinned output or explicit unresolved state if pinned
render cannot be reproduced. This forces resolver/reproducibility decisions
before claiming live continuity.

Next distinct acceptance gate: explicit refresh A->B retains layer edits and
marks dirty; delayed A render cannot publish over B. Automatic live follow,
continuous Develop preview, mixed geometry/camera support, complex stacks and PSD
copy interoperability require subsequent agreed contracts and acceptance.

No tests/builds/apps/benchmarks or heartbeat restart on B. Only documentation and
git diff checks performed; no feature or performance acceptance claimed.
