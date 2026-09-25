# Catalog Storage & Lightroom Import

Decision: **we do not adopt the `.lrcat` model.** Lightroom Classic's catalog is a single monolithic SQLite file that is the sole source of truth, with previews and AI pixel data in separate side-directories, a single-writer lock, an opaque schema, and a corruption story that only got auto-repair in 2026. We keep what is good about it (SQLite is fine as an *index*) and change what is not.

## 1. Design goals

1. **Files are the source of truth, the database is a cache.** Losing the database loses nothing that cannot be rebuilt.
2. **Portable and inspectable.** Edits travel with the photos; a folder copied to another disk or machine opens with its edits intact. Everything is plain text where practical.
3. **No import ceremony.** Open a folder and start working; "importing" only means indexing.
4. **Fast at scale.** 1M+ images, instant grid scroll, sub-second search, no "optimize catalog".
5. **Multi-machine / multi-user safe.** No single-writer lock on a shared volume; conflict-free merging of metadata edits.
6. **First-class Lightroom Classic import** with high-fidelity edit translation.

## 2. Storage model

```
photos/2026/09/wedding-smith/
  IMG_0001.CR3
  IMG_0001.CR3.xmp          ← standard XMP sidecar (rating, label, keywords, IPTC, crs:* develop keys)
  .edits/IMG_0001.json      ← our full edit recipe (superset of crs:*; masks, AI refs, history)
  .edits/IMG_0001/masks/*.webp   ← cached AI mask rasters (regenerable)
  .edits/IMG_0001/pixels/*.dng   ← cached pixel edits (remove/denoise patches), regenerable
  .folder.json              ← folder-level metadata: colour tag, sort order, default preset
~/Library/Application Support/<app>/index/
  index.sqlite              ← rebuildable index (WAL, FTS5), one per user
  previews/<hash>/*.jxl     ← preview pyramid, content-addressed
  embeddings.lance          ← vector index for similarity/NL search (LanceDB or SQLite+vec)
~/.../library.json          ← lightweight "library" file: list of root folders, collections, saved searches, settings
```

- **Per-image state** lives in two files next to the image: a standard XMP sidecar for interoperability (any other app can read ratings/keywords/basic develop keys) and a JSON recipe for everything the XMP model cannot express. For DNG/JPEG/TIFF/PSD we still write XMP into the file only if the user opts in; default is sidecar so originals are never touched.
- **Recipe JSON** is versioned, append-only history with named snapshots, deterministic hash of the "current" state used as the render cache key.
- **Library file** (`library.json`) holds cross-image structures: collections, saved searches, people names, keyword hierarchy, publish state. It is small and can be synced with any file sync tool or git.
- **Index** is SQLite (WAL, `synchronous=NORMAL`, memory-mapped, FTS5 for text, R*Tree for GPS). It is fully rebuildable by scanning roots and reading sidecars + embedded metadata. Tables: `image`, `file`, `folder`, `metadata` (EXIF/IPTC/XMP flattened), `keyword`/`image_keyword` (closure table), `face`, `person`, `score`, `collection`/`collection_image`, `recipe_hash`, `preview`.
- **Embeddings** (CLIP-class image embeddings, face embeddings, perceptual hashes) in a vector store for similarity grouping, duplicate detection and natural-language search, all local.
- **Concurrency**: sidecar writes are atomic (write temp + rename) with a per-file last-writer-wins + vector clock in the JSON; the index is per-machine so shared volumes never contend on a database lock. Optional LAN/cloud sync replays sidecar changes (see [06-culling-and-selection.md](06-culling-and-selection.md) for sync of decisions).
- **Why not DuckDB/Postgres/etc.**: SQLite is embedded, zero-ops, and fast enough for an index; the ergonomic problem with `.lrcat` was that it was the only copy of the data, not that it was SQLite.

### 2.1 Folder-first browsing ("no import")
Opening any folder indexes it lazily: embedded JPEG previews render instantly (Photo Mechanic-style), full previews and AI scores fill in the background. "Add to library" just pins the folder as a root so it is indexed persistently and searchable.

### 2.2 Previews
Content-addressed by file hash + orientation, stored as JPEG XL pyramids (1/8 … 1/1). Smart previews (2560 px lossy DNG) generated on demand for offline editing. Cache size limits with LRU eviction; previews are never required for correctness.

## 3. Lightroom Classic catalog import

Goal: a user points at a `.lrcat` (or a folder of images with `.xmp` sidecars) and gets folders, collections, keywords, metadata, flags/ratings/labels, faces, virtual copies, stacks, history snapshots, and develop edits that **render visually identical** for the supported Process Versions.

### 3.1 What we read from `.lrcat` (SQLite)
| Data | Tables |
|---|---|
| Roots, folders, files | `AgLibraryRootFolder`, `AgLibraryFolder`, `AgLibraryFile` |
| Images, virtual copies, orientation, capture time | `Adobe_images` (`masterImage`, `copyName`, `orientation`, `captureTime`, `pick`, `rating`, `colorLabels`) |
| Develop settings (XMP text) | `Adobe_imageDevelopSettings` (`text`, `processVersion`, `hasDevelopAdjustments`), `Adobe_AdditionalMetadata` (`xmp` blob) |
| History & snapshots | `Adobe_libraryImageDevelopHistoryStep`, `Adobe_libraryImageDevelopSnapshot` |
| EXIF/IPTC | `AgHarvestedExifMetadata`, `AgLibraryIPTC`, `AgInternedExifCameraModel`, `AgInternedExifLens` |
| Keywords | `AgLibraryKeyword` (hierarchy via `parent`), `AgLibraryKeywordImage`, synonyms in `AgLibraryKeywordSynonym` |
| Collections | `AgLibraryCollection` (`creationId` distinguishes smart/regular/sets/publish), `AgLibraryCollectionImage`, `AgLibraryCollectionContent` (smart rules as serialized Lua) |
| Stacks | `AgLibraryFolderStack`, `AgLibraryFolderStackImage`, `AgLibraryCollectionStack*` |
| Faces/people | `AgLibraryFace`, `AgLibraryFaceCluster`, `AgLibraryKeywordFace` (person keywords) |
| GPS / places | `AgHarvestedExifMetadata` (gps), `AgLibraryPlace*` |
| Publish services | `AgLibraryPublishedCollection*`, `AgRemotePhoto` |
| AI pixel data | `<catalog>.lrcat-data` (Generative Remove / Lens Blur / Denoise blobs) |
| Previews | `Previews.lrdata` pyramid files (optional import to avoid regenerating) |

Import runs read-only against a copy of the catalog (Lightroom must not be running or we copy the file first) and writes our sidecars + library file. Smart-collection rule Lua is parsed into our saved-search grammar (rating, flag, label, keyword, date, camera, lens, text ops, and/or/none nesting).

### 3.2 Develop-settings translation
- The `crs:` XMP namespace is our recipe's compatibility layer: every key in Adobe's `crs` schema (Exposure2012, Contrast2012, Highlights2012, Shadows2012, Whites2012, Blacks2012, Texture, Clarity2012, Dehaze, Vibrance, Saturation, Temperature, Tint, ToneCurvePV2012 (+R/G/B), ParametricShadows…, HueAdjustmentRed…, SaturationAdjustment…, LuminanceAdjustment…, SplitToning/ColorGrade*, Sharpness/SharpenRadius/SharpenDetail/SharpenEdgeMasking, LuminanceSmoothing/ColorNoiseReduction…, LensProfile*, ChromaticAberration*, Defringe*, PerspectiveUpright/Vertical/Horizontal/Rotate/Aspect/Scale/X/Y, CropTop/Left/Bottom/Right/Angle, PostCropVignette*, Grain*, CameraProfile, Look, RedHue/RedSaturation… calibration, MaskGroupBasedCorrections (masks), RetouchAreas/RetouchInfo, PointColors, LensBlur, Enhance/Denoise, HDR…) maps to a recipe field.
- We implement Adobe **Process Versions 3–6 math** in a compatibility mode so imported edits render the same; new edits default to our own native pipeline (see [07-image-quality-and-color.md](07-image-quality-and-color.md)). PV1/2 are converted with a best-effort mapping and flagged.
- Masks: geometric masks and range masks translate exactly; AI masks (subject/sky/people/objects/landscape) are re-run with our models, and a diff badge marks images whose masks changed materially.
- Generative Remove / Lens Blur pixel results are imported from `.lrcat-data` where present so results do not change; otherwise re-executed with our local models.
- Camera profiles: Adobe Standard/Color/Landscape/etc. are DCP files shipped with Camera Raw; we map to our equivalent profiles and expose an "Adobe-look compatibility" profile set built from the same DCP data where licensing allows, otherwise from our own camera characterization ([07 §3](07-image-quality-and-color.md)).
- Validation: importer renders a sample of images through both the compat pipeline and reports ΔE statistics against Lightroom's embedded previews so the user sees fidelity before committing.

### 3.3 Other import sources
- Capture One catalogs/sessions (`.cocatalog`/`.cosessiondb` SQLite; edits translated partially, adjustments layer-mapped), Photo Mechanic IPTC, Apple Photos (PhotoKit export of originals + adjustments as our recipe where possible), darktable (`.xmp` with `darktable:history`), plain folders with XMP.

## 4. Export & interop
- Always write standard XMP alongside our recipe so Lightroom/Bridge/Photo Mechanic/Capture One see ratings, labels, keywords, IPTC and basic develop keys.
- "Export catalog for Lightroom" produces a folder with XMP sidecars using `crs:` keys plus rendered TIFF/DNG for edits Lightroom cannot express.
