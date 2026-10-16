# Offline editing: the drive can be unplugged (2026-10-16)

Owner's ask (Rut, 2026-10-16): "why can't I edit the smart previews when the SSD isn't connected?"

## What happens today, and why

- Tessera has a complete Smart Preview engine (`crates/tessera-ffi/src/smart_preview.rs`,
  `pipeline-cpu/src/smart_preview.rs`): a camera-linear proxy at 2048 px (f16 + zstd, ~10 MB
  for a 33 MP ARW, ~30 s to build on an M4 Max of which most was lock wait, see PF-1), stored
  under `~/Library/Application Support/Tessera/smart-previews/<image id>/{pixels.tsp,
  journal.json}`, with a journal of local edits and a sync step that publishes them to the
  sidecars when the original is back.
- **Nothing builds them.** The only entry point is Library ▸ Smart Previews ▸ Build… for the
  selected photos, off by default ("Use Smart Previews When Available" is false). Rut's
  app-support folder had no `smart-previews` directory at all.
- When the drive is unplugged the folder opens as a "Cached Smart Preview Library": catalog
  read-only, rows only for photos that have a local preview, so Rut saw "No cached Smart
  Previews … declarations" and a title of "No folder". Even with previews present, Edit would
  refuse until the person found the menu toggle ("Original offline. Choose Library → Smart
  Previews → Use Smart Preview…").
- The 19 733 photos under `~/Pictures/Lightroom/… Smart Previews.lrdata` are the imported
  Lightroom catalog's own lossy-DNG previews (rows flagged `lightroom_smart_preview`). They are
  local and open without the drive; this spec is about photos scanned from the drive itself
  (226 today, the whole SSD once he opens the rest).

## The experience we want

1. **It just works.** While a drive is attached, Tessera makes an offline preview for every RAW
   in the folders you open, in the background, newest and kept photos first, inside a disk
   budget. Nothing to turn on.
2. **Unplug and keep editing.** With the drive gone, the folder still opens, Edit opens the
   offline preview without a menu trip, the loupe says "Editing offline preview", and edits are
   journaled locally.
3. **Plug it back in and it catches up.** The next time the original is reachable (folder open
   or volume mount) the journaled edits are written to the sidecars automatically; a toast says
   how many. Conflicts stay visible in the Smart Previews menu and never lose a copy.
4. **You can see where you stand.** The folder popover says "180 of 226 editable offline";
   Settings ▸ Offline shows the mode, the budget, the space used and a "Make this folder
   available offline now" button.

Size honesty: 10 MB per photo is 2.3 GB for the 226-photo folder and ~200 GB for 20 k photos.
The budget defaults to 10 % of the free space on the boot volume, capped at 25 GB, kept and
newest photos first; the Settings tab says what fits. A smaller or lossy tier is follow-up
OF-4, not a reason to ship nothing.

## Work packages

| WP | Scope | Owner/branch |
| --- | --- | --- |
| PF-1 | `Index::roots()` counted files per root with a `substr` scan over every file row: 54 s under the catalog lock on every drive flush and folder open. Rewritten as two index range probes (46 ms). Done on main (`crates/index/src/roots.rs`). | main |
| OF-1 | Engine API for coverage, candidates, idempotent build, pending-sync listing, storage; bindings; tests. | `of1-engine` |
| OF-2 | App: background builder with budget, Settings ▸ Offline, folder-popover coverage, automatic routing to the preview when the original is offline, automatic sync on reconnect, offline-library copy. | `of2-app` |
| UX-15 | Chrome collisions and copy found in today's screenshots (separate brief, `docs/design/redesign-2026-10/FOLLOWUPS.md` §UX-15). | `ux15-polish` |
| OF-3 | Offline decisions: keep/reject/rating while the drive is detached, written to the index and replayed to the sidecars on reconnect (`Core::open_owned_declared` is read-only today). | follow-up |
| OF-4 | Smaller preview tier (1600 px or a log-encoded 8-bit payload) with an explicit error policy, so "every RAW" fits a laptop. | follow-up |

## OF-1: engine contract (uniffi, `crates/tessera-ffi/src/offline.rs`)

```rust
#[derive(uniffi::Record)]
pub struct OfflineCoverage {
    pub folder: String,
    pub raws: u64,          // mosaic RAW originals under `folder` (recursive, same rule as the cull session)
    pub ready: u64,         // of those, with a local preview that is not stale/conflicting
    pub pending_sync: u64,  // local previews with journaled edits not yet written to the sidecars
    pub needs_attention: u64, // stale, conflict, failed
    pub bytes: u64,         // on-disk size of the previews counted in `ready` + `pending_sync` + `needs_attention`
}

impl Engine {
    /// Cheap: file list from the reader connection, then stat/journal reads with no catalog lock held.
    pub fn offline_coverage(&self, folder: String) -> Result<OfflineCoverage>;
    /// RAW image ids under `folder` without a usable local preview, ordered: keep decisions first,
    /// then newest capture first. `limit` 0 means all.
    pub fn offline_candidates(&self, folder: String, limit: u32) -> Result<Vec<String>>;
    /// Returns the existing preview's info when one is present and usable; otherwise builds it.
    /// Never holds the catalog lock across decode/encode (build_smart_preview already takes it only
    /// in `original_path`; keep it that way). A refusal (non-mosaic source, lens opcode list 3,
    /// denoise on) is an error with the engine's message, not a panic.
    pub fn ensure_smart_preview(&self, image_id: String) -> Result<SmartPreviewInfo>;
    /// Ids whose journal has edits not yet synchronized. Scans `smart-previews/<image id>/journal.json`.
    pub fn pending_sync_smart_previews(&self) -> Result<Vec<String>>;
    /// Bytes under `smart-previews/`.
    pub fn smart_preview_storage(&self) -> Result<u64>;
}
```

Rules: no new state tables; everything derives from the journal files and the index. Tests in
`crates/tessera-ffi/tests/offline.rs` with the existing RAW fixtures (see
`tests/smart_preview_workflow.rs` for how a build is driven in tests): coverage before/after a
build, candidates ordering, ensure idempotence, pending list after a journaled save and after
synchronize, storage sum. Regenerate the Swift bindings (`apps/mac/Support/build-ffi.sh`, see
how CS-1 did it) so `TesseraFFI.swift` carries the five methods.

## OF-2: app contract

- `OfflinePreviews` (`apps/mac/Sources/Tessera/Library/OfflinePreviews.swift`, `@Observable`,
  main actor): owns the queue for the open library. Starts 3 s after the folder's cull
  measurement finishes (or 3 s after load when there is nothing to measure), one build at a
  time via `EngineCall.detached(priority: .background)`, re-checks the budget before each,
  pauses while `developStatus == .loading`, while an export/print is running, while a Smart
  Preview batch from the menu is running, and for 2 s after any loupe frame; resumes on its own.
  Cancels when the library changes. Exposes `coverage`, `isBuilding`, `current`, `budget`,
  `lastError`.
- Mode (`UserDefaults "OfflinePreviewsMode"`): `.everyRaw` (default), `.keepersOnly`, `.off`.
  Budget (`"OfflinePreviewsBudgetBytes"`): default `min(25 GB, 10 % of free space on the boot
  volume)`. Stops with coverage text "budget reached" when `storage + 12 MB > budget`.
- Routing (`SmartPreviewRouting.route`): original offline + preview usable → `.smartPreview`
  with no toggle. Missing → "“<volume>” isn't attached and this photo has no offline preview yet.
  Previews are made in the background while the drive is attached (Settings ▸ Offline)."
  `model.developSourceRoute == .smartPreview` shows "Editing offline preview" in the Edit header
  line, not a menu-only badge.
- Reconnect: `AppModel` runs `syncPendingOfflineEdits()` after a folder opens with its original
  available and after a volume mounts: `pending_sync_smart_previews()` ∩ library ids →
  `synchronize_smart_preview` one by one off-main → toast "Wrote 3 offline edits to the drive"
  (or "1 edit could not be written · see Library ▸ Smart Previews"). Never while a Develop
  session on that photo is open.
- Offline library copy (`LibraryAccessMode`, `WorkspaceHeader`, `ContentView` empty state,
  `folderTitle`): title is the folder name; banner "“Rut photo” isn't attached · editing offline
  previews · culling resumes when it's back"; empty state "No offline previews for this folder
  yet. Attach “Rut photo” and Tessera makes them in the background." No "declarations",
  "cached", "catalog read-only" wording anywhere the person can see.
- Folder popover: the current folder row and detached roots carry "180 of 226 editable offline"
  as the detail (coverage from OF-1; refreshed when the builder finishes a photo).
- Settings ▸ Offline (`Settings/OfflineSettingsView.swift`, after Sync in `TesseraApp`): mode
  picker, budget stepper in GB with "fits about N photos", "Used: 2.3 GB · 226 previews",
  "Make this folder available offline now" (runs the queue ignoring the idle gates, still inside
  the budget), and a one-line status of the builder.
- Errors shown to people come from `localizedDescription`; `BridgeError` gets
  `LocalizedError` with `errorDescription` = the message (UX-15 adds the conformance; use it).

Tests: `OfflinePreviewsTests` with a fake `SmartPreviewAPI` (the controller is written against
an injected API like `SmartPreviewController`): gates, ordering, budget stop, cancel on library
change, auto-route when offline, pending sync toast counts, settings defaults. ThemeLint stays
green.

## Human steps

None. Everything is on-device; no portal work.
