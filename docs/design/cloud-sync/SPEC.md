# Tessera everywhere: the catalog follows the drive and the account (2026-10-15)

Owner's ask (Rut): "keep my catalog in the cloud or something so I can attach my hard drives /
SSDs to any computer with my account and all my stuff will load."

## What "all my stuff" is, and where it lives today

| State | Today | Portable? |
| --- | --- | --- |
| Photos | the drive | yes |
| Develop edits, decisions (keep/reject/grade), marks, keywords on a photo | XMP sidecar next to the photo (`crs:*`, `xmp:Rating`, `dc:subject`, `ts:*`) | yes |
| Albums, album groups, smart albums | `library.json` in the opened folder | yes |
| People (face clusters, names), faces | index `person` / `face` tables in `~/Library/Application Support/Tessera` | **no** |
| Analysis scores, captions, OCR, embeddings | index `score` / `understanding` / embedding tables | no (rebuildable, hours for 20k photos) |
| Culling learner, style profile | app support JSON keyed by a hash of the folder path | **no** |
| AI preferences, appearance | UserDefaults / `ai-preferences.json` | no |
| Previews, mask cache | app support | no (rebuildable, slow) |
| Image identity | `blake3(absolute path)` | only while the mount point is identical |

The storage spec (docs/05) already says the index is per-machine and fully rebuildable from the
roots plus the sidecars. So the design is not "put the SQLite in the cloud" (SQLite over a file
sync corrupts, and the index is 5 GB here). It is two moves:

1. **Drive-resident state.** Everything that belongs to a set of photos lives next to them, in a
   `.tessera/` folder at the root of the opened folder (or the volume), so attaching the drive to
   any Mac brings it along. The index on each Mac is a cache built from it.
2. **Account-resident state.** Everything that belongs to the photographer, not to a drive, lives
   in the iCloud Drive container of the app (the user's Apple ID is the account; no server to run).

## 1. Identity that survives a different mount point (CS-1)

- Roots are remembered by **volume UUID + path relative to the volume** (`DADiskCopyDescription`
  / `URLResourceKey.volumeUUIDStringKey`), with the last mount point as a hint. A root whose volume
  is not attached shows in the folder popover as "Rut photo · not attached" with its counts; it
  opens the moment the volume mounts, wherever macOS mounts it (`/Volumes/Rut photo-1` included).
- Image ids become `blake3(volume-uuid + "/" + relative-path)` for files on removable or external
  volumes, and stay path-based for the boot volume (migration: a one-time rebuild of the per-machine
  index on first launch; sidecars carry the truth, so nothing is lost). engine-api contract bump.
- `library.json` keys that reference images by id keep working because ids are now the same on
  every Mac the drive visits.

## 2. Drive-resident state in `.tessera/` (CS-1)

At the root of each opened folder (today's `library.json` location):

```
.tessera/
  library.json            albums, groups, smart albums (moved from the folder root; old path read once)
  people.json             person ids, names, merged/split clusters, cover face; face rows by image id
  analysis/               scores, captions, OCR, understanding; one JSON per image id, written as computed
  learning.json           culling learner state for this shoot
  style-profile.json      style profile learned from this shoot
  previews/               optional compact preview cache (JPEG pyramid, the same keys as the index)
```

Rules: atomic writes (temp + rename), last-writer-wins by `modified` timestamp and machine id per
record, never a lock. Opening a folder reads `.tessera/` into the per-machine index before the
scan completes, so people, scores and albums are there immediately; the scan only fills gaps.
Hidden folder; a user who deletes it loses nothing that cannot be recomputed except names
(people) and albums, which is why both also sync to the account (§3).

## 3. Account-resident state in iCloud Drive (CS-2)

Ubiquity container `iCloud.dev.tessera.app`, folder `Tessera/`:

```
preferences.json        AI provider choice (never keys), appearance, culling preferences
keywords.json           the keyword hierarchy
style-profile.json      the photographer's global style profile (shoot profiles merge into it)
learning.json           the global culling learner
people/<id>.json        people names and merged cluster ids (face embeddings stay on the drive)
drives/<volume-uuid>.json  the drive's roots, display name, counts, last seen on which Mac
```

- Sign-in is the Mac's iCloud account; Tessera shows "Synced as <name>" in Settings ▸ Sync with
  what syncs and when it last did, and an off switch. No account: everything still works locally.
- Merge: records carry `modified` + `machine`; last writer wins per record; people merges are
  union-find over cluster ids so two Macs naming the same person converge.
- Writes go through `NSFileCoordinator`; reads through a metadata query so changes made on another
  Mac apply live (the index applies them through the existing change feed).
- Secrets (API keys) stay in the Keychain; the Keychain's own iCloud sync carries them if the user
  allows it. Never in the container.

## 4. What the user sees

- Attach a drive on a second Mac, open Tessera: the folder popover already lists it (from
  `drives/`), opening it shows albums, people, decisions, scores and edits at once; previews
  render in the background (or load from `.tessera/previews` if the drive carries them).
- Settings ▸ Sync: account, on/off, "Keep previews on the drive" toggle, last sync, storage used.
- A one-line status in the folder popover: "Rut photo · 226 photos · synced 2 min ago".

## 5. Work packages

| WP | Scope | Touches |
| --- | --- | --- |
| CS-1 | Volume-relative identity, roots by volume UUID, `.tessera/` drive-resident state (people, analysis, learning, style profile, library.json move), index rebuild on migration | crates/index, crates/library, crates/tessera-ffi catalog + assist, engine-api contract, TesseraCore EngineLibrary/LibraryCatalog, folder popover |
| CS-2 | iCloud Drive container, account-resident state, merge rules, Settings ▸ Sync, drives list in the popover | apps/mac entitlements + Info.plist, a new TesseraCore/Sync module, Settings |
| CS-3 | Preview cache on the drive, smart-preview style, with a size budget | crates/previews, Settings |

Human prerequisites for CS-2 (Rut, once): in the Apple Developer portal create the iCloud container
`iCloud.dev.tessera.app` on the app id `dev.tessera.app`, enable iCloud (CloudKit + iCloud
Documents) on it, and create a **Developer ID provisioning profile** with iCloud for the app;
`make-app.sh` embeds it as `Contents/embedded.provisionprofile` when `Support/release/Tessera.provisionprofile`
exists. Without it the app runs with sync disabled and says so in Settings.
