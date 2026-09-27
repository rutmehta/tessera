# B5-14 needs (for Machine A: crates/compositor, crates/jobs)

B5-14 did not edit crates/compositor or crates/jobs. These engine APIs would remove the two workarounds in
`crates/tessera-ffi/src/document/render.rs` and `document.rs`.

## 1. A lineage-preserving document snapshot (compositor)

`Document::clone()` assigns a new cache key (`next_doc_key()`). The FFI now keeps the live document in an
`Arc` shared copy-on-write with the frame being rendered (`CowDoc`, P14). When an edit lands while a frame
still holds the snapshot, `Arc::make_mut` clones the document, so the live document gets a new key. The next
frame then cannot use the resident damage log (`resident::damage`: `last.key == doc.key()`):

- if the program changed (opacity, blend mode, visibility, adjustment parameters) the viewport is
  recomposited in full instead of only the damaged blocks;
- smart-object pages keyed by `SmartKey::doc` are resampled again;
- CPU-compositor tiles keyed by `doc.key()` are recomputed (styled documents already key root tiles by
  revision, so they lose nothing extra).

The FFI keeps the window short (the snapshot is dropped before the GPU wait), so this only happens when an
edit races the CPU side of a frame or a CPU (style) frame. Wanted: `Document::fork_same_lineage(&self) -> Document`
(same key, history, epoch and damage log; documented as "for immutable snapshots of one session"), or making
`Document` internally `Arc`-backed so a cheap snapshot shares the key. Either lets the FFI replace
`Arc::make_mut` with a lineage-preserving copy.

## 2. A public interactive-pressure guard (jobs)

`jobs::pressure::begin/end` are `pub(crate)`: only jobs queued on a `ThreadPoolScheduler` count. Document
frames run on the session's render thread and filter bakes on their own worker, so the FFI registers them by
queuing a Viewport-priority "hold" job on a small private pool for the duration of the work
(`render::Pressure`), recycling the pool every 4096 holds because a scheduler keeps completed statuses for its
lifetime. Wanted: `pub struct InteractiveGuard` (`jobs::interactive_guard() -> InteractiveGuard`, begin on
create, end on drop) so non-scheduler interactive work registers directly, without threads or retained records.

## 3. (Nice to have) the output region in `FrameReport` (compositor)

Tests assert the rendered region from the union of `FrameReport::damage` on a fresh renderer. A
`FrameReport::region` (the block-aligned level-buffer window) would let every frame, not only a cold one,
report what storage the level covers.
