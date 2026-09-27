# B5-08 implementation status: persistent alpha and spot Channels panel

## FFI (crates/tessera-ffi/src/document/channels.rs, `impl DocumentSession`)

Every edit is one existing `DocOp` (`AddChannel`, `DeleteChannel`, `RenameChannel`, `EditChannel`, or
`SetSelection` through `apply_selection`) and therefore one labelled history node.

| Call | Returns | History label |
| --- | --- | --- |
| `document_channels()` | `Vec<ChannelRecord{id, kind: DocChannelKind(Alpha/Spot), name, color: PaintColor, opacity, visible, index, revision}>` | – |
| `save_selection_channel(name, target: Option<u64>, op: SelectionOp)` | `ChannelUpdate{channel_id, update}` | Save Selection |
| `load_selection_channel(id, op, invert)` | `DocumentUpdate` | Load Selection |
| `new_alpha_channel(name, selected)` (extra: footer "New Channel" and Quick Mask without a selection) | `ChannelUpdate` | New Channel |
| `rename_document_channel(id, name)` | `DocumentUpdate` | Rename Channel |
| `delete_document_channel(id)` | `DocumentUpdate` | Delete Channel |
| `duplicate_document_channel(id)` ("<name> copy", appended) | `ChannelUpdate` | Duplicate Channel |
| `set_spot_channel(id, color, solidity)` (validated 0…1, finite; an alpha channel becomes spot) | `DocumentUpdate` | Channel Options |
| `new_spot_channel(name, color, solidity, from_selection)` | `ChannelUpdate` | New Spot Channel |
| `channel_thumbnail(id, max_px)` | RGBA8 IOSurface id (grey, box-filtered), cached per channel samples | – |
| `set_channel_visible(id, visible)` | `()` (session preview state, not history, not saved) | – |

Legacy `save_selection(name)` / `load_selection(name, op)` / `selection_channels()` keep their signatures and now act on
the persistent channels: save replaces the first alpha channel of that name (or adds one), load uses the first channel
of that name, `selection_channels` lists every channel name in order (duplicates included; ids tell them apart).

Revisions / thumbnail cache: a per-session side table (visible ids, sample-identity → revision numbers, thumbnail
surfaces) keyed by the session's shared state, compared with `Raster::shares_all_tiles_with`, so undo / redo return
the same revision and thumbnail.

## Persistence

* Native `.tessera-doc`: channels are `DocState::channels` and already serialised by compositor `format.rs` (id,
  name, kind incl. spot colour / solidity, raster at its own depth, `next_channel_id`). Reopen keeps ids.
* PSD: compositor `psd.rs` writes extra composite planes plus resources 1006 (names), 1007 / 1077 (display info: alpha
  = red 50 %, spot = colour + solidity %, mode 2). Reopen gives the same names, kinds, spot colour (16-bit quantised)
  and solidity (whole percent), planes at the document depth; ids are renumbered 1…n.
* Spot channels never enter the RGB composite or flat export (tested byte-for-byte, also after a PSD round trip).
* Not saved: channel visibility, alpha overlay colour / opacity / indicator (session preferences in the app; PSD
  always records alpha as red 50 %).

## App

* TesseraCore/Document/Channels: `DocumentChannelsBackend` (third protocol), `SavedChannel` records, pure models
  (`ChannelsPanelModel`, `SaveSelectionForm`, `LoadSelectionForm`, `QuickMask`), engine adoption, stub adoption
  (rectangular channels, session-only).
* Tessera/Document/Channels: `DocumentChannels` (model / actions), `ChannelsPanel`, `ChannelSheets` (Save / Load
  Selection, Channel Options, New Spot Channel), `ChannelOverlay` (viewport preview: component eyes, alpha overlays,
  spot inks), `ChannelsSelfTest` (env `TESSERA_CHANNELS_SELFTEST=<dir>`).
* Hooks (delimited `B5-08 begin/end`): DocumentView.swift (Channels section, sheets, attach), ToolsMenus.swift (Save /
  Load Selection…, Quick Mask item), ToolsSheets.swift (`.saveSelection` → channel sheet; old session sheet removed),
  KeyRouter.swift (Q). EngineDocumentBackend.swift / StubDocumentBackend.swift untouched (adoption lives in extension
  files in the new directory).

## Tests

* Rust `crates/tessera-ffi/tests/document_channels_ui.rs`: 10 tests, `test result: ok. 10 passed; 0 failed`.
  Every SelectionOp × invert on load, every op on save-into-channel, duplicate names, stale ids (deleted, 0, unknown)
  on every call, undo / redo of each op, invalid spot metadata (no history recorded), `.tessera-doc` and PSD round
  trips, RGB export unchanged by spot / alpha channels (also after PSD), thumbnails / visibility / revisions.
  Existing `document_tools.rs` channel checks still pass through the routed legacy calls.
* Swift `DocumentChannelsTests`: 7 tests (panel model, component eyes / names, Save and Load sheet mapping, Quick
  Mask on the stub, stub channel ops, engine adapter incl. Quick Mask and undo). Full `swift test`: 193 tests, 0
  failures.
* Gate: `cargo test -p compositor -p selection -p psd -p tessera-ffi --release` all ok; clippy `-D warnings` clean;
  `cargo fmt --check` clean; build-ffi, swift build, swift test, xcodebuild Debug: BUILD SUCCEEDED.
* App run (engine, scratch PNG of sample.dng): `channels-selftest … done, 0 failure(s)`; screenshots in `evidence/`.

## Deviations

1. tools.rs: besides the approved `pub(super)` on `apply_selection`, the three legacy method bodies are one-line
   delegations to channels.rs, and the session-local `channels` BTreeMap field (and its now-unused `BTreeMap` import)
   was removed (approved by the coordinator; the brief assumed the legacy calls lived in document.rs).
2. Calls that create a channel return `ChannelUpdate { channel_id, update }` instead of a bare id, so the host gets
   the `DocumentUpdate` like every other edit. Added `new_alpha_channel(name, selected)` for the footer's New Channel
   and Quick Mask without a selection.
3. Channel eyes are a host overlay (ChannelOverlay.swift) over the viewport, not a renderer change (render.rs is
   B5-07's). It draws from channel / composite thumbnails at up to 4096 px, so very large canvases preview at reduced
   resolution. R / G / B eyes are real (components removed from the composite thumbnail; one visible component shows
   as grey).
4. Quick Mask: the brush engine cannot paint into channels (tools.rs off limits), so the temporary channel is edited
   with Save Selection into it (Add / Subtract / Intersect); entering is 1–2 history nodes, leaving 2 (Load Selection,
   Delete Channel).
5. Alpha overlay colour / opacity / masked-vs-selected indicator are session preferences (the compositor's
   `ChannelKind::Alpha` carries no display metadata).
6. The stub's channels are rectangles and not part of stub history (no thumbnails).

## Noticed outside scope

* `DocumentTools.refreshOutline` (DocumentTools.swift) can apply a stale outline: an in-flight fetch for a selection
  finishes after the selection was cleared, because clearing does not bump `outlineToken`. Seen once as leftover ants
  in a self-test screenshot; not fixed (outside allowed paths).
