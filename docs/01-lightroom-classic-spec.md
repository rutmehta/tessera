# Lightroom Classic — Feature Specification & Implementation Guide

Version baseline: Lightroom Classic 15.x (2025–2026 release train), Camera Raw 18.x.
Companion docs: [Photoshop spec](02-photoshop-spec.md) · [Shared engine architecture](04-implementation-architecture.md) · [Competitive analysis](03-competitive-analysis.md)

Each feature below follows one template:

- **What it does** — user-facing behaviour and controls
- **Parameters / data model** — the state that must be persisted (all Develop state is stored as XMP-style key/value "develop settings" and is non-destructive)
- **Implementation** — the algorithms, data structures and pipeline stage needed to build an equivalent
- **Dependencies** — what other subsystems it relies on

The implementation notes assume the shared engine described in [04-implementation-architecture.md](04-implementation-architecture.md): a scene-referred, linear-light, floating-point raw pipeline with tiled, GPU-accelerated rendering; an SQLite catalog; and a non-destructive edit-recipe model.

---

## 0. Product architecture overview

| Layer | Lightroom Classic | Notes for an implementation |
|---|---|---|
| Storage | Files stay on disk; catalog (`.lrcat`, SQLite) stores metadata + develop recipes + previews pointers | Never move/modify originals except on explicit user command |
| Previews | `Previews.lrdata` (pyramidal JPEG), `Smart Previews.lrdata` (lossy DNG ~2560 px) | Multi-resolution cache keyed by image ID + edit hash |
| Render | Adobe Camera Raw (ACR) engine, shared with Photoshop's Camera Raw filter | Same recipe renders identically across apps: essential for LrC ↔ Ps round-trip |
| Modules | Library, Develop, Map, Book, Slideshow, Print, Web | Modal module UI with shared filmstrip & catalog |
| Sync | Optional sync of collections to Lightroom cloud via Smart Previews | One catalog may sync at a time |
| Extensibility | Lua SDK (plug-ins, publish services, export filters, metadata tagsets), Tethering plug-ins | Sandboxed Lua interpreter with UI toolkit bindings |

---

## 1. Catalog & Library module

### 1.1 Catalog

> Our implementation departs from the `.lrcat` model; see [05-catalog-storage-and-import.md](05-catalog-storage-and-import.md). This section documents Lightroom's behaviour for parity and import.

**What it does.** A single-file database of every imported photo: file path, capture metadata, user metadata, develop settings, history, collections membership, flags. Supports Open/Create/Optimize/Back up, multiple catalogs, "Import from Another Catalog" merge, "Export as Catalog" subset extraction, catalog upgrade between versions.

**Data model.**
- `Image` (id, root folder, relative path, file format, capture time, orientation, pixel dims, fileSize, hash, missingFlag)
- `Metadata` (EXIF, IPTC, XMP namespaces; stored normalized + as a raw XMP packet blob)
- `DevelopSettings` (current recipe JSON/XMP), `DevelopHistory` (ordered list of steps), `Snapshots`
- `Collection`, `CollectionSet`, `SmartCollection` (rule tree), `CollectionImage` (M:N)
- `Keyword` (hierarchical, synonyms, export flags), `ImageKeyword`
- `Folder` (root + hierarchy mirrored from filesystem), `Volume`
- `Preview` pointers, `SmartPreview` pointers, `PublishService`, `PublishedCollection`, `PublishedPhoto`

**Implementation.**
- SQLite with WAL journaling; one writer, many readers; all metadata edits batched in transactions.
- Full-text index (FTS5) across filename, caption, title, keywords, camera/lens fields for the Library text filter.
- Write-ahead journal + periodic auto-backup (zip + integrity check via `PRAGMA integrity_check`).
- "Optimize catalog" = `VACUUM` + `ANALYZE` + rebuilding indexes.
- Catalog import/merge: deterministic UUID per image (from original file hash + capture time) so merges dedupe.
- Missing-file detection: store volume UUID + relative path; on volume mount, reconcile via path, then fall back to hash search on "Locate".
- Undo/redo is per-module: Library metadata edits go on a global undo stack; Develop edits are history steps.

**Dependencies.** XMP reader/writer, filesystem watcher (optional; LrC does not watch, it uses "Synchronize Folder").

### 1.2 Import

**What it does.** Import dialog with sources (folders, cards, devices, tethered); modes Add / Copy / Copy as DNG / Move; file renaming templates; apply develop preset + metadata preset + keywords on import; build previews (Minimal / Embedded & Sidecar / Standard / 1:1); build Smart Previews; "Don't Import Suspected Duplicates"; second-copy backup; destination folder organization by date templates; import presets; auto-import from watched folder.

**Implementation.**
- Card/device enumeration via PTP/MTP and mounted-volume scan; grid thumbnails from embedded JPEG previews (fast path) without full decode.
- Duplicate detection: (original filename, capture time, file size) tuple, plus optional content hash.
- Rename templates = token grammar (`{Date(YYYYMMDD)}_{Sequence(0001)}_{OriginalFilename}`) shared with Export.
- Copy as DNG: raw → DNG conversion (lossless JPEG or lossy JPEG-XL/JPEG tiles, embed original optional, fast-load data, preview sizes).
- Previews pipeline queued as background jobs at lower thread priority; embedded-preview mode displays camera JPEG until a true render is built.
- Watched folder: filesystem watch (FSEvents / ReadDirectoryChangesW / inotify) + debounce + move-into-destination.

### 1.3 Folders panel & file management

**What it does.** Filesystem tree per volume with photo counts, colored labels for folders, favorites, drag-move of files/folders (performs real filesystem move), rename, "Synchronize Folder", "Show Parent Folder", "Add Subfolder", volume status (free space, online/offline).

**Implementation.** Mirror filesystem hierarchy into `Folder` table; moves are transactional (move file → update DB → write XMP), rollback on failure. Volume identity via UUID (macOS `diskutil`/`statfs`, Windows volume serial), so external drives reconnect at new mount points.

### 1.4 Grid / Loupe / Compare / Survey / People views

**What it does.**
- Grid: thumbnail cells with badges (crop, develop, keywords, GPS, collection), rating/flag/label overlays, stacks, expanded/compact cells, custom sort orders (capture time, edit time, rating, pick, label, filename, aspect ratio, custom drag order in collections).
- Loupe: fit/fill/1:1/zoom levels, info overlays (two configurable templates), navigator panel, view-options.
- Compare: candidate/select with synchronized zoom/scroll, swap.
- Survey: N images side-by-side with dismiss.
- People: face-region clustering, named/unnamed tabs, drag-to-name, confirm suggestions.
- Secondary display support (Loupe/Grid/Compare/Live Loupe/Locked).

**Implementation.**
- Virtualized grid with LRU thumbnail cache; requests coalesced by visible viewport; thumbnails from preview pyramid level closest to cell size.
- Loupe: tile-based renderer that draws from the preview pyramid until the Develop engine delivers a fresh render for the current recipe hash.
- Custom sort: per-collection `order` float column with gap re-numbering.
- Faces: detector (RetinaFace-class CNN) → alignment → 512-d embedding (ArcFace-class) → agglomerative clustering under a distance threshold; store `FaceRegion` (normalized rect, embedding, personId, confidence, confirmed flag). Writes MWG-RS region XMP so other apps see names.

### 1.5 Flags, ratings, color labels, stacks, virtual copies

> Our selection model replaces flags/ratings/labels; see [06-culling-and-selection.md](06-culling-and-selection.md). Stacks and virtual copies are kept as-is.

**What it does.** Pick/Reject/Unflagged; 0–5 stars; Red/Yellow/Green/Blue/Purple + custom label sets; Auto-Stack by capture time; manual stacks (collapse/expand/promote); Virtual Copies (extra develop recipes on the same file); "Set as Master".

**Implementation.**
- Flags/ratings/labels as integer columns + XMP mapping (`xmp:Rating`, `xmp:Label`, `lr:...`).
- Stacks: `Stack(id, collectionId nullable)` + `StackMember(imageId, position)`; auto-stack = single pass over sorted capture times with threshold.
- Virtual copies: `Image` rows sharing `masterFileId` with `copyName`; render pipeline keys the cache on (fileId, recipeHash), so VCs are free until edited.

### 1.6 Collections, Collection Sets, Smart Collections, Quick Collection, Target Collection

**What it does.** Manual collections (nested in sets), smart collections with rule trees (any/all/none, nested), Quick Collection (B key), target collection, per-collection sort, sync-to-cloud toggle, "Set as Target".

**Implementation.**
- Smart collection rules compiled to SQL `WHERE` clauses (text ops, ranges, date relative "in the last N days", "has adjustments", "is in collection", keyword containment via closure table). Cache results; invalidate on metadata write affecting referenced columns.
- Keyword hierarchy stored as adjacency list + closure table for fast "contains descendant" queries.

### 1.7 Keywording & keyword list

**What it does.** Hierarchical keywords with synonyms, "Include on Export" / "Export Containing Keywords" / "Export Synonyms" flags, person keywords, keyword sets (recent, custom 9-key sets), keyword suggestions (co-occurrence based), painter tool (spray can), import/export of keyword lists (tab-indented text), filter by keyword.

**Implementation.** Suggestions = co-occurrence matrix weighted by time proximity of capture. Export mapping flattens hierarchy into `dc:subject` (flat) + `lr:hierarchicalSubject` (path with `|` delimiters).

### 1.8 Metadata panel & presets

**What it does.** Editable EXIF/IPTC/XMP fields (default, all, EXIF, IPTC, IPTC Extension, Large Caption, Location, Minimal, Quick Describe, Video, DNG, custom tagsets via plug-in), metadata presets, copyright status, sync metadata across selection, "Save Metadata to File", "Read Metadata from File", auto-write XMP option, metadata conflict badges, capture-time edit (shift by offset / set), EXIF editing of lens/camera fields (limited), metadata status filter (up to date / changed on disk / conflict).

**Implementation.**
- XMP toolkit (or equivalent) to read/write sidecars for proprietary raw, embed XMP in DNG/JPEG/TIFF/PSD.
- Conflict detection: compare file mtime + stored XMP hash against DB; surface badge.
- Capture-time edit rewrites `DateTimeOriginal` in DB and XMP; only writes into raw EXIF if user opts into "write date to proprietary raw".

### 1.9 Library filter bar & filter presets

**What it does.** Text (any searchable field, rules: contains/doesn't contain/starts/ends), Attribute (flag, rating ≥/≤/=, label, kind: master/VC/video), Metadata columns (date, camera, lens, label, keyword, ISO, aperture, focal length, shutter, GPS, treatment, develop preset, edit status, file type, location, ...), locked filters, filter presets, "None". Combined with collection scope and folder scope.

**Implementation.** Compose SQL from three filter groups joined with AND; metadata column panes are faceted counts (`GROUP BY`) computed on the currently scoped set.

### 1.10 Quick Develop

**What it does.** Relative (incremental) adjustments in Library: preset, treatment, WB, tone controls in coarse/fine steps, crop ratio, "Sync Settings", "Sync Metadata". Applies to many images relative to each image's current value.

**Implementation.** Apply deltas to each image's recipe (not absolute values); queue re-render of thumbnails.

### 1.11 Previews & Smart Previews

**What it does.** Standard preview size/quality choices, 1:1 preview auto-discard age, "Build/Discard 1:1 Previews", Smart Previews (lossy DNG, ≤2560 px long edge) for offline editing and for "Use Smart Previews instead of Originals for image editing" (speed), preview cache limits (Camera Raw cache separate).

**Implementation.**
- Pyramid: JPEG levels at 1/1, 1/2, 1/4 … stored in a per-image blob file; index in `previews.db`.
- Smart Preview: demosaiced, downscaled, lossy-compressed DNG with full develop metadata so the same recipe applies; on export the original is used if online.
- Camera Raw cache: caches early-stage pipeline output (post-demosaic, pre-tone) keyed by (file hash, process version, profile) so re-renders on tone changes skip demosaic.

### 1.12 Publish Services & plug-ins

**What it does.** Publish to Hard Drive, Flickr, Adobe Stock, third-party services via Lua SDK; tracks "New photos to publish / Modified photos to re-publish / Published"; comments sync. Plug-in manager. Export filters, metadata tagsets, Library menu commands.

**Implementation.** Publish state machine per photo: `NEW → PUBLISHED → MODIFIED (recipe/metadata hash changed) → PUBLISHED`. Lua sandbox exposes: catalog access (`LrCatalog`), tasks (`LrTasks`), dialogs (`LrDialogs`/`LrView` bindings), HTTP (`LrHttp`), export session hooks, develop settings read/write.

### 1.13 Sync with Lightroom cloud

**What it does.** Collections marked for sync upload Smart Previews + metadata + develop settings; edits made in Lightroom mobile/web/desktop sync back; conflicts resolved "last write wins" per field. Only one catalog can sync.

**Implementation.** Change-log based sync: each entity change gets a monotonic revision; client uploads deltas and pulls deltas since last cursor. Develop settings are synced as full XMP snapshots (no CRDT), with capture-time-based conflict UI.

---

## 2. Develop module — the raw pipeline

> SOTA upgrades to every stage below, plus camera/lens coverage and display colour management, are specified in [07-image-quality-and-color.md](07-image-quality-and-color.md).

The Develop module UI panels, in order: Histogram · Tool strip (Crop, Healing, Redeye, Masking, Lens Blur) · Basic · Tone Curve · HSL/Color · Color Grading · Detail · Lens Corrections · Transform · Effects · Calibration. Plus: Presets, Snapshots, History, Collections on the left; Before/After, Soft Proofing, Reference View, Copy/Paste/Sync/Auto Sync, Previous, Reset in the toolbar.

### 2.0 Pipeline stage ordering (implementation)

Adobe's "Process Version" (PV) defines the fixed order and math. An equivalent scene-referred pipeline:

1. **Decode** raw container (TIFF/EP-based CR2/CR3/NEF/ARW/RAF/ORF/RW2/DNG etc.) → mosaic data + black/white levels + WB coefficients + color matrices + lens metadata.
2. **Linearize** (black subtraction, white point, per-channel scaling, DNG `LinearizationTable`), **pixel-defect** mapping, **highlight reconstruction** (clip-aware, propagate from unclipped channels).
3. **Demosaic** (Bayer: AMaZE/RCD/DCB-class; X-Trans: Markesteijn-class; DNG linear raw skipped).
4. **Lens corrections** (geometric distortion, chromatic aberration, vignetting) — operate early to keep everything downstream in corrected coordinates. Defringe after.
5. **Camera → working space**: apply forward matrix / camera profile (DCP: ColorMatrix + ForwardMatrix + HueSatDelta LUT + LookTable + ToneCurve); working space = linear ProPhoto-primaries (RIMM) float.
6. **White balance** (temperature/tint → multipliers in camera space, applied before matrix).
7. **Noise reduction & sharpening (Detail panel)** in linear light; AI Denoise runs on the mosaic *before* demosaic (see 2.9).
8. **Tone**: Exposure → Contrast → Highlights/Shadows/Whites/Blacks (adaptive, local-histogram aware) → Parametric+Point tone curve → Dehaze/Clarity/Texture (frequency-separated locals).
9. **Color**: Vibrance/Saturation → HSL / Point Color → Color Grading (3-way) → Calibration (primaries).
10. **Local adjustments (Masks)** — each mask is a parameterized adjustment layer applied in linear light with the mask as alpha; order-independent additive deltas except for local tone curves.
11. **Effects**: post-crop vignette, grain (applied at output resolution).
12. **Transform / Upright / Crop** — geometric resample (Lanczos-3) once, combined with any lens geometry into a single warp to avoid double resampling.
13. **Output**: HDR or SDR tone mapping (see 2.15), output color transform (ICC), output sharpening (screen/matte/glossy) and resize, then encode.

All stages implemented as GPU compute kernels on 32-bit float tiles (e.g., 512×512 with halo), with a CPU fallback. Recipe changes re-run only the dirty stage and downstream (memoize stage outputs per tile).

### 2.1 Histogram & clipping indicators

**What it does.** Live RGB histogram of the rendered output; shadow/highlight clipping overlays (J key); shows original vs. Smart Preview status, and HDR range when in HDR mode. Draggable regions map to Blacks/Shadows/Exposure/Highlights/Whites.

**Implementation.** Compute 256-bin (or 1024 in HDR) histogram on the display-resolution render via GPU atomic adds; clipping mask = per-channel threshold on the pre-output-transform buffer.

### 2.2 Basic panel

#### Treatment & Profile
- Color / Black & White toggle; profile browser (Adobe Raw: Adobe Color, Adaptive Color, Monochrome, Adaptive B&W, Landscape, Neutral, Portrait, Standard, Vivid; Camera Matching; Creative: Artistic, B&W, Modern, Vintage; Legacy) with "Amount" slider for creative profiles; favorites.
- **Implementation.** DCP profiles (`ColorMatrix1/2`, `ForwardMatrix`, `HueSatMap`, `LookTable`, `BaseToneCurve`); creative profiles = DCP + 3D LUT (`.cube`-like, applied in a defined space) with amount blending. **Adaptive profiles** (Adaptive Color / Adaptive B&W) are image-dependent: a small CNN predicts a per-image tone/color transform (or a low-res base adjustment map) that is applied as a hidden layer before user sliders; results are cached per image. Implement as: downscale to ~512 px → network outputs a 3D LUT + a spatially-varying tone mask (guided-upsampled to full res).

#### White Balance
- Presets (As Shot, Auto, Daylight, Cloudy, Shade, Tungsten, Fluorescent, Flash, Custom), Temperature (2000–50000 K), Tint (−150..+150), eyedropper with loupe.
- **Implementation.** Map (T, tint) → CIE xy chromaticity (Planckian locus + Duv offset) → adapted white in camera space via inverse camera matrix → per-channel multipliers. "Auto" = gray-world/illuminant estimation, optionally CNN-based. Eyedropper: average 5×5 region in camera-linear, solve multipliers that neutralize it.

#### Tone (Auto, Exposure, Contrast, Highlights, Shadows, Whites, Blacks)
- Exposure ±5 EV (linear gain in scene space with a soft highlight roll-off near white); Contrast (S-curve about mid-gray in a perceptual space); Highlights/Shadows (range-limited, edge-aware local operators — bilateral/guided-filter-based luminance masks that avoid halos); Whites/Blacks (end-point adjustment with soft clipping); **Auto** (ML-predicted slider values trained on human edits; in modern versions it also sets Vibrance/Saturation and Color Mix).
- **Implementation.** Order matters: Exposure in linear → build "base" luminance with guided filter (radius ~ 1–2% of image, ε tuned) → Highlights/Shadows modulate a gain curve on the base layer only, preserving detail layer → Whites/Blacks adjust end points on the curve → Contrast. Auto: a regression net on a 256-px thumbnail + histogram features outputs slider values; fall back to histogram-stretch heuristic.

#### Presence (Texture, Clarity, Dehaze, Vibrance, Saturation)
- **Texture**: mid-high frequency contrast (±100) without affecting fine noise or large edges. Implementation: bandpass = image − guided-filter-smoothed image at small radius, minus noise-sized frequencies; add scaled band back.
- **Clarity**: local mid-tone contrast at larger radius (unsharp mask on luminance using a wide radius with a mid-tone weighting mask; halo suppression via edge-aware base).
- **Dehaze**: estimate atmospheric light + transmission map (dark-channel prior, refined with guided filter); positive removes haze, negative adds; correct color shift via WB-aware airlight.
- **Vibrance**: saturation boost weighted inversely by current saturation and protecting skin-tone hues; **Saturation**: uniform chroma scale in a perceptual space (e.g., Oklab/IPT-ish).

### 2.3 Tone Curve

**What it does.** Parametric (Highlights/Lights/Darks/Shadows with movable split points), Point curve (RGB + per-channel R/G/B), curve presets (Linear/Medium/Strong contrast), point editing with target adjustment tool (TAT), "Refine Curve" sliders.

**Implementation.** Parametric = four-region weighted gamma curves blended with smoothstep across split points. Point curve = monotone cubic (Fritsch–Carlson) spline sampled to a 4096-entry LUT applied in a display-referred encoding (sRGB-like gamma of the working space); per-channel curves applied after the RGB curve. TAT: sample luminance under cursor, drag maps to curve output delta at that input.

### 2.4 HSL / Color / B&W mix, Point Color

**What it does.** Eight hue bands (Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta) each with Hue/Saturation/Luminance sliders; B&W mix (per-band luminance) when treatment is B&W; TAT. **Point Color**: pick up to 8 arbitrary colors; per swatch adjust Hue/Saturation/Luminance with Range (hue/sat/lum spread) and falloff; visualize range overlay. Point Color is also available inside masks.

**Implementation.** Convert to a hue-linear perceptual space; each band = smooth weight function of hue (raised-cosine overlapping windows). Apply hue rotation, chroma scale, lightness scale weighted by band membership. Point Color: weight = product of Gaussian falloffs in hue distance × saturation × luminance around the picked value with user-set ranges; apply HSL deltas; overlay = weight map. B&W mix: gray = Σ weight_band(h) · L · (1 + slider/100).

### 2.5 Color Grading

**What it does.** Three-way (Shadows/Midtones/Highlights) color wheels + Global wheel; each with Hue, Saturation, Luminance; Blending (overlap between ranges) and Balance (shift the tonal split point). Replaces Split Toning.

**Implementation.** Luminance weights for shadows/mid/high = three overlapping smooth windows whose overlap width = Blending and whose center = Balance; add chroma vector (hue, sat) in an opponent space scaled by weight and by a luminance-preserving term; Luminance slider offsets tone within the window.

### 2.6 Detail panel — Sharpening

**What it does.** Amount (0–150), Radius (0.5–3), Detail (0–100), Masking (0–100), with Alt-preview overlays.

**Implementation.** Capture sharpening in linear luminance: unsharp mask with deconvolution-like Detail control (Detail blends between halo-suppressed USM and a high-frequency-boosting deconvolution term); Masking = edge mask from Sobel magnitude, thresholded and blurred, gating where sharpening applies. Runs before NR mixing according to Adobe order (sharpen & NR interleaved on the same frequency decomposition).

### 2.7 Detail panel — Noise Reduction (manual)

**What it does.** Luminance (amount, Detail, Contrast), Color (amount, Detail, Smoothness).

**Implementation.** Multi-scale (wavelet/à-trous or NL-means-lite) luminance denoise with an ISO-derived noise profile (per camera, per ISO: read/shot-noise variance model). Color NR: chroma channels denoised more aggressively in a decorrelated space (e.g., YCbCr-like), with Smoothness controlling large-scale chroma blotch removal (large-radius chroma bilateral). Detail slider = threshold on wavelet coefficient shrinkage; Contrast preserves local contrast by protecting high-variance regions.

### 2.8 Lens Corrections

**What it does.** Profile tab: Remove Chromatic Aberration, Enable Profile Corrections (auto lens profile from EXIF, manual make/model/profile choice, Amount sliders for Distortion and Vignetting), setup default. Manual tab: Distortion (with "Constrain Crop"), Defringe (Purple/Green amount + hue range with eyedropper), Vignetting (Amount/Midpoint). Built-in corrections for mirrorless lenses read from raw opcode metadata (DNG opcodes / manufacturer embedded).

**Implementation.**
- Lens Correction Profiles (LCP XML): Brown–Conrady radial + tangential distortion polynomial as a function of focal length & focus distance; lateral CA as per-channel radial scale polynomials; vignetting as a radial polynomial gain; interpolate across focal-length/aperture samples.
- Apply distortion + CA as a single inverse-mapped resample (part of the final warp when possible; otherwise once early). Vignetting applied in linear light as gain.
- Auto CA removal: per-channel radial alignment estimation on edges across the frame (estimate R and B radial scale relative to G by minimizing edge misalignment), no profile needed.
- Defringe: detect high-chroma pixels of the selected hue near strong luminance edges and desaturate toward neighbor luminance.
- DNG opcode lists (WarpRectilinear, FixVignetteRadial, etc.) executed at the DNG-specified stage.

### 2.9 Enhance: Denoise (AI), Raw Details, Super Resolution

**What it does.**
- **Denoise**: AI noise reduction on the raw mosaic; Amount slider; outputs a new DNG (Enhanced) or—as of recent versions—applies *in-place non-destructively* to the raw with no DNG needed; supports Bayer & X-Trans, and HDR merges/pano DNGs.
- **Raw Details**: improved demosaic for fine detail and fewer artifacts; produces Enhanced DNG.
- **Super Resolution**: 2× linear upscale (4× pixels) into Enhanced DNG.

**Implementation.**
- Denoise: U-Net-style CNN operating on 4-channel packed Bayer (RGGB) at half resolution with the noise level as a conditioning input (from ISO/noise profile), trained on paired clean/noisy raw; Amount blends network output with input. Run in tiles with overlap on GPU (Core ML / DirectML / CUDA). For "no DNG" mode, cache the denoised mosaic in the Camera Raw cache and treat as a pipeline stage keyed by (file, amount).
- Raw Details: learned demosaic (CNN) replacing the classical demosaic stage; output linear RGB.
- Super Resolution: ESRGAN-class 2× network on demosaiced linear RGB; store as linear-raw DNG with the same develop metadata so all edits remain non-destructive.

### 2.10 Transform panel (Upright)

**What it does.** Upright modes: Off, Auto, Level, Vertical, Full, Guided (draw 2–4 guide lines); manual sliders Vertical, Horizontal, Rotate, Aspect, Scale, X/Y Offset; "Constrain Crop"; grid overlay; "Update" after changing lens profile.

**Implementation.** Line segment detection (LSD/Hough) → cluster into vanishing points (RANSAC on line intersections, use lens-corrected coordinates) → build a homography that maps chosen vanishing points to infinity (Vertical: make vertical VP go to infinity; Level: rotate to make horizon horizontal; Full: both plus aspect correction; Auto: constrained version limiting perspective strength). Guided: user lines directly define VPs. Compose homography with lens distortion and crop into one resampling pass.

### 2.11 Effects panel

**What it does.** Post-Crop Vignetting (Style: Highlight Priority / Color Priority / Paint Overlay; Amount, Midpoint, Roundness, Feather, Highlights), Grain (Amount, Size, Roughness).

**Implementation.** Vignette = radial mask in crop-normalized coordinates with superellipse roundness; Highlight Priority applies in linear light (so highlights punch through), Color Priority in a perceptual space with chroma preserved, Paint Overlay = blend with black/white. Grain = procedurally generated noise texture (multi-octave, seeded) applied at output resolution, luminance-only with size-dependent blur and roughness-controlled contrast; must be resolution-independent (scale by output pixel size).

### 2.12 Calibration panel

**What it does.** Process Version selector (1, 2, 3, 4/5, 6 = current); Shadows tint; Red/Green/Blue primaries Hue & Saturation.

**Implementation.** Modify the matrix mapping camera RGB → working space by rotating and scaling each primary's chromaticity; effect propagates to all downstream colors ("calibration hue shifts"). Process Version = versioned pipeline math; older PVs must be preserved for backward-compatible rendering of old edits.

### 2.13 Crop & Straighten, Red Eye

**What it does.** Crop with aspect presets/custom, lock aspect, rotate, flip, straighten tool (drag a line) and auto (from Upright Level), overlays (thirds, golden ratio, spiral, grid, diagonals, aspect ratios) cycled with O, constrain-to-image when transformed; crop keyboard nudges. Red Eye: pupil size, darken; Pet Eye with catchlight.

**Implementation.** Crop is stored as normalized rectangle + angle in image space; rendering applies crop as the last stage of the combined geometric warp. Red eye: localized detection of high-red-saturation blob within click radius; desaturate + darken pupil, keep specular.

### 2.14 Healing: Content-Aware Remove, Heal, Clone, Generative Remove

**What it does.** Brush-based spots/strokes with Size, Feather, Opacity; modes Content-Aware Remove (patch synthesis, with "Refresh" for a new seed), Heal (source blend), Clone (source copy), manual source repositioning, "Visualize Spots" (edge-emphasized view with threshold), "Detect Objects" toggle for Remove; **Generative Remove**: cloud-based generative inpainting with 3 variations, invisible object-shadow removal; stack of edits are all non-destructive and re-executable.

**Implementation.**
- Store each stroke as a mask path + parameters; healing is re-executed at render time at the working resolution (cache results per zoom level).
- Heal: PatchMatch or similar to find a source patch, then gradient-domain (Poisson) blend seamlessly. Clone: direct copy with feathered alpha.
- Content-Aware Remove: PatchMatch-based inpainting with structure propagation; "Detect Objects" runs a segmentation to exclude object pixels from the source pool; Refresh changes the random seed.
- Generative Remove: send masked crop (+ context) to a diffusion inpainting service; receive N variants; cache in catalog as pixel patches (stored in an `.lrcat-data` companion) so re-rendering does not re-call the service. Content Credentials are attached to exports that include generative pixels.

### 2.15 Masking (local adjustments)

**What it does.** Masks panel with mask types:
- **AI**: Subject, Sky, Background, Objects (brush or rectangle → segmentation), People (per person, with Face Skin, Body Skin, Eyebrows, Eye Sclera, Iris & Pupil, Lips, Teeth, Hair, Clothes as separate sub-masks), Landscape (Water, Architecture, Vegetation, Natural Ground, Artificial Ground, Mountains).
- **Geometric**: Brush (size, feather, flow, density, auto mask), Linear Gradient, Radial Gradient (feather, invert).
- **Range**: Color Range (eyedropper samples + refine), Luminance Range (min/max with feather), Depth Range (from depth map when available, e.g. iPhone HEIC / dual-pixel DNG).
- Mask arithmetic: Add / Subtract / Intersect with any other mask type; Invert; duplicate; rename; show overlay (red, green, white, black-on-white, image-on-black, etc.); mask amount slider (fade).
- Per-mask adjustments: Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Texture, Clarity, Dehaze, Hue (with "Use fine adjustment"), Saturation, Temp, Tint, Color tint, Point Color, Sharpness, Noise, Moiré, Defringe, Curve (local tone curve), Grain, Amount (mask opacity).
- Adaptive presets store the AI mask and adjustments; masks can be copied/synced (AI masks are recomputed on target images).

**Implementation.**
- Masks stored as *procedural definitions* (not pixels) where possible: brush strokes as spline + radius/flow, gradients as parameters, range masks as thresholds; AI masks stored as compressed low-res alpha (e.g., 1024-px max PNG/RLE) + version of the model so they can be regenerated; combine at render time.
- Segmentation models: Subject/Background (salient object segmentation, e.g. U²-Net class), Sky (semantic), People (person instance + face/body-part parsing network), Objects (SAM-style promptable segmentation from box/brush), Landscape (multi-class semantic segmentation). Run at ~1024 px, refine with guided-filter upsampling against the full-res image to produce soft edges.
- Mask arithmetic: alpha compositing tree evaluated per tile: `add = max(a,b)` (or screen), `subtract = a·(1−b)`, `intersect = a·b`.
- Adjustments in each mask are the same operators as the global panel applied with alpha blending in linear light; local curves and Hue use LUTs interpolated by alpha.
- Luminance range: compute on the *pre-local* tone; Color range: distance in a perceptual color space with feather; Depth range: read depth map (HEIC auxiliary image / DNG depth map / Portrait mode), normalize, threshold.

### 2.16 Lens Blur

**What it does.** AI-generated depth map → synthetic shallow depth of field: Blur amount, Bokeh shape (circle, bubble, 5-blade, ring, cat eye), Boost (highlight bloom), focal range selection (drag in histogram-like range bar or click subject), "Visualize Depth", refine with brush (focus / blur). Sub-modes: Subject focus, Point focus.

**Implementation.** Monocular depth estimation network (e.g. DPT/MiDaS-class) at ~1024 px; store depth map with the recipe; blur = depth-dependent variable-kernel rendering (layered depth-of-field: quantize depth into slices, blur each with the chosen aperture kernel size = f(|depth − focus|), composite back-to-front with occlusion-aware alpha). Boost = extract highlights above threshold, spread with the kernel, screen-blend.

### 2.17 Presets, Profiles, Snapshots, History

**What it does.** Presets (folders, favorites, Adaptive presets that include AI masks, premium/style presets with Amount slider, import/export `.xmp`, "Presets: Amount"), Snapshots (named, timestamped), History (every step, with clear/before), Before/After views (left/right, top/bottom, split), Copy/Paste settings dialog with checkboxes, Sync, Auto Sync, "Previous", Reset (with modifier for partial resets), "Match Total Exposures", Reference View (second image for matching).

**Implementation.** Presets = partial recipes (only chosen keys) merged onto current; "Amount" scales each numeric key linearly between current and preset (curves interpolated pointwise). History = append-only list of recipes (delta-encoded); Snapshot = named full recipe. Match Total Exposures: compute EV from EXIF (aperture, shutter, ISO) for each image and offset Exposure to equalize.

### 2.18 Soft proofing

**What it does.** Toggle soft proof; choose output profile (sRGB, Adobe RGB, printer ICCs), rendering intent (perceptual/relative), simulate paper & ink, gamut warnings (destination/monitor), "Create Proof Copy".

**Implementation.** ICC transform via CMM (Little-CMS class) to output profile then back to display profile with black point compensation; gamut warning = round-trip difference > ΔE threshold, monitor gamut via display profile.

### 2.19 HDR editing & display

**What it does.** HDR toggle in Develop: edits in HDR with headroom (up to +4 EV highlight range), HDR histogram, "Visualize HDR" overlay, "Preview for SDR display" with SDR rendition controls (Brightness, Contrast, Highlights, Shadows, Whites, Blacks, Clarity), HDR-capable export (AVIF, JPEG XL, JPEG with gain map, PNG HDR/16-bit, TIFF float) and HDR display on supported monitors/Apple XDR.

**Implementation.** Keep the pipeline scene-referred with unbounded float; when HDR mode is on, disable the SDR tone mapper and map to display using PQ/HLG via the OS HDR path (Metal EDR / DirectX HDR swapchain). SDR rendition = a secondary tone map with its own sliders producing the SDR base for gain-map export (gain map = log2(HDR/SDR) per pixel, stored per Adobe/ISO 21496-1 gain map spec).

### 2.20 Photo Merge: HDR, Panorama, HDR Panorama

**What it does.**
- HDR: merge bracketed raws to 32-bit float DNG; Auto Align, Auto Settings, Deghost (none/low/medium/high) + "Show Deghost Overlay"; "Create Stack".
- Panorama: projection (Spherical, Cylindrical, Perspective), Boundary Warp (0–100), Fill Edges (content-aware), Auto Crop, "Create Stack", batch merge via headless mode.
- HDR Panorama: combined.

**Implementation.**
- HDR: align via feature matching (ORB) + homography per frame; estimate relative exposure from EXIF; merge in linear raw space (weighted average with well-exposedness weights) → float DNG (linear raw DNG, mosaic-free) with the same develop metadata; deghosting via reference-frame consistency test (pixels whose predicted radiance deviates beyond noise model are taken from reference only).
- Pano: SIFT/ORB features → pairwise matches → bundle adjustment of camera rotations + focal lengths → project to chosen surface → seam finding (graph cut) → multiband (Laplacian pyramid) blending; Boundary Warp = mesh-based warp that stretches the panorama to fill the bounding rectangle (thin-plate spline on boundary constraints); Fill Edges = content-aware fill of transparent regions.

### 2.21 Tethered capture

**What it does.** Tethered capture for Canon, Nikon, Sony, Fujifilm (via plug-in), Leica: live view (with overlays), remote shutter, settings readout (shutter, aperture, ISO, WB), auto-advance, develop settings on import, session naming/segmentation, destination, "Add to collection".

**Implementation.** Camera SDKs (Canon EDSDK, Nikon SDK, Sony Camera Remote SDK) or PTP/IP; background thread pulls captured files, ingests via the Import pipeline with preset application; live view = MJPEG frames rendered into a view with focus-peaking/overlay options.

### 2.22 Export

**What it does.** Export dialog: location (specific folder, same as original, choose later), subfolder, add to catalog/stack; file naming templates; video export settings; format (JPEG quality/limit file size, PSD, TIFF (compression, 8/16-bit), PNG, DNG (compat, lossy, embed original), AVIF, JPEG XL, Original), color space (sRGB, AdobeRGB, ProPhoto, Display P3, Rec.2020, custom ICC), HDR output toggle, image sizing (W×H, dimensions, long/short edge, megapixels, percentage, don't enlarge, resolution ppi), output sharpening (screen/matte/glossy, low/std/high), metadata (all/copyright/copyright+contact/all except camera/…, remove person info, remove location info, write keywords as hierarchy), watermarking (text/graphic with anchor, opacity, size, inset), post-processing (show in Finder, open in app, export actions), export presets, multiple presets in one export, plug-in export filters. Also "Export with Previous" and "Email".

**Implementation.** Export = final render at target size: full-res render → Lanczos resample → output sharpening (radius scaled by ppi/output type) → ICC transform → encode. Batch executor with N-way parallelism bounded by memory. Watermark composite in output space. Metadata writer maps chosen privacy filters.

### 2.23 Map module

**What it does.** Map (Google/Adobe tiles), photo pins & clusters, drag photos to map to geotag, GPS track log import (GPX) with time-offset auto-tagging, reverse geocoding (City/State/Country/Sublocation suggestions), saved locations with privacy radius, location filter.

**Implementation.** Tile map view; GPX interpolation by capture time (with camera clock offset); reverse geocoding via a geocoding API, cached; private locations strip GPS on export.

### 2.24 Book module

**What it does.** Blurb/Amazon-style book layouts, page templates, auto layout presets, text, captions from metadata, backgrounds, PDF/JPEG export, upload to Blurb.

**Implementation.** Layout templates as JSON with cells; render pages via the same export renderer; PDF via a PDF writer with ICC-embedded images.

### 2.25 Slideshow module

**What it does.** Templates, overlays (text, rating, identity plate), backdrop, soundtrack (multiple tracks), "Sync slides to music", pan & zoom, transitions, export to PDF or video (H.264 up to 1080p).

### 2.26 Print module

**What it does.** Layout styles (Single Image/Contact Sheet, Picture Package, Custom Package), margins, cell sizes, rulers/guides, identity plates, overlays, page setup, color management (managed by printer or by Lightroom with ICC + intent), 16-bit output (macOS), print sharpening, print resolution, "Print to JPEG", print adjustment (brightness/contrast), print templates, soft-proof link.

**Implementation.** Page composition engine → high-res render of each cell → ICC transform to printer profile → OS print pipeline (CUPS / Windows print spooler), with the option of driver-managed color (pass sRGB/AdobeRGB tagged data).

### 2.27 Web module

**What it does.** HTML gallery templates, site info, color palette, appearance, image sizes, upload via FTP.

### 2.28 Video support

**What it does.** Import, playback, trimming, poster frame, capture frame, limited Quick Develop-style adjustments (WB, tone, treatment via presets), export with settings. (Lightroom cloud has fuller video editing; Classic remains limited.)

**Implementation.** Decode via OS media frameworks; adjustments implemented as a LUT applied on decode for preview and on transcode for export.

### 2.29 Performance features

**What it does.** GPU acceleration for display, image processing, and export; "Use Smart Previews for editing"; preview cache sizing; Camera Raw cache; "Generate previews in parallel"; multi-core export; Apple silicon / Windows ARM native; "AI-accelerated" features use NPU/GPU.

### 2.30 Content Credentials & Generative provenance

**What it does.** Attach C2PA Content Credentials on export (creator, connected accounts, edits & activity, generative AI usage).

**Implementation.** C2PA manifest builder: assertions for actions (`c2pa.edited`, `c2pa.opened`, `c2pa.placed`, ai-generative assertions), hash of the exported asset, signed with an Adobe/CA certificate; embedded in JPEG/PNG/WebP/AVIF/TIFF containers.

---

## 3. Recent additions (2024–2026) to keep parity with

Dated changelog with sources: [research/adobe-current.md](research/adobe-current.md). Items an implementation must cover beyond the sections above:

| Area | Feature (version) | Implementation pointer |
|---|---|---|
| Healing | Generative Remove GA with Detect Objects (14.0); Distraction Removal: People, Reflections (14.4), Dust (15.0); updated model (14.5) | §2.14; reflection removal = single-image reflection/transmission separation network producing two layers |
| Detail | Denoise & Super Resolution non-destructive in Detail panel, no DNG (14.4); Denoise on Apple Neural Engine (15.4); Topaz Generative Upscale 2×/4× (15.2, credits) | §2.9; treat as cached pipeline stage |
| Profiles | Adaptive Color / Adaptive B&W (14.2), film-inspired presets & profiles (15.3) | §2.2 |
| Masking | Landscape masks (14.3), Snow (15.0), Feather & Edge sliders on AI masks (15.5), faster brush, background AI processing for copy/paste/sync (15.3), Bulk Delete Empty Masks | §2.15 |
| Colour | Point Color Variance (15.0) | §2.4: variance widens the hue/sat/lum Gaussian jointly |
| Lens Blur | GA with batch/sync (13.3); oval/anamorphic bokeh + cat-eye slider (14.1) | §2.16: anisotropic kernel and radial cat-eye clipping |
| HDR | HDR in Library views (13.5); ISO 21496-1 gain-map export (14.0); HDR Limit slider 1–8 (15.0); separate SDR/HDR Edit-in-Photoshop (15.0) | §2.19 |
| Library | Limit Preview Cache (14.0); GPU preview generation (14.5); Assisted Culling + Faces panel (15.0 EA → 15.4 GA); Auto-Stack by visual similarity (15.0); Duplicate Detection (15.4); keyword sync to cloud (15.4); filter by web likes/comments; milliseconds in capture time | §1.4–1.6; culling = sharpness/eye-open/eye-focus classifiers per face + global aesthetics score; similarity = perceptual-hash + embedding clustering |
| Catalog | Rename Catalog, streamlined upgrade (14.0); Backups tab (14.2); lrcat-data corruption auto-detect & repair (15.5) | §1.1 |
| Export/Files | Content Credentials EA (14.0); PSB export & Edit-in (15.1); WebP import/edit (15.2); Batch Rename in Export (15.2); 4K video export (15.0); Render to DNG (15.5, bakes AI edits, stackable Denoise + Super Res) | §2.22, §2.30 |
| Tethering | Sony (13.3) and expanded models; native Apple-silicon Nikon (14.0); click-to-focus Live View (14.2); native Fujifilm, Canon R1/R5 II (14.4); Leica (15.1) | §2.21 |
| Camera Raw only (so far) | Glow (Diffusion/Bloom/Halation, maskable), Trim (transparent outside crop), Metadata panel (ACR 18.6) | Glow = thresholded highlight extraction → large Gaussian/Lorentzian bloom → screen blend, with halation tinted red/orange |
| Lightroom cloud only | Quick Actions, Generative Expand (Classic status disputed), natural-language search, Edit using Describe, Adaptive Landscape presets, interactive histogram, 10 custom color labels, video editing | Roadmap candidates |

## 4. Implementation priority matrix (suggested)

| Tier | Features | Why first |
|---|---|---|
| P0 (core) | Catalog, import, previews, raw decode + demosaic + DCP profiles, WB, Basic tone, curve, HSL, sharpening/NR, lens profiles, crop/straighten, export, XMP | Minimum viable non-destructive raw workflow |
| P1 | Masking (geometric + range), presets/snapshots/history, Color Grading, Point Color, Upright, Effects, healing (heal/clone), HDR/Pano merge, soft proof, print | Feature parity for enthusiasts |
| P2 | AI masks (subject/sky/people/objects/landscape), AI Denoise, Super Resolution, Content-Aware Remove, Lens Blur, Adaptive profiles, face recognition, tethering | Differentiators that need ML infrastructure |
| P3 | Generative Remove, Reflection Removal, HDR display/export, Content Credentials, sync/cloud, Book/Slideshow/Web, Lua plug-in SDK | Ecosystem & platform features |
