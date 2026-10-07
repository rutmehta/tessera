# import-lrcat

`import(path) -> EngineResult<ImportPlan>` reads a Lightroom catalog without
writing it or its photos. `inspect(path) -> EngineResult<Summary>` validates the
same plan and returns counts. `plan.library.write(path)` explicitly writes an
atomic serde `library.json` document. No sidecars are written by inspection.

## Safety and representation

- Every catalog is copied to temporary storage before SQLite opens it, including
  its WAL if present. SQLite opens the copy with URI `mode=ro` and read-only
  flags. This also handles Lightroom lock files without touching them. Changes
  detected during copying fail with a retry/close-Lightroom error. The source
  must remain quiescent during the copy; this is not an online SQLite backup API.
- Missing core tables produce named `EngineError::Decode` errors. Optional
  metadata tables missing in older catalogs are listed in `plan.report`.
  Additional columns are accepted.
- Virtual copies retain the master's actual photo path and receive separate
  recipe identities and display names. Identity is derived from the canonical
  catalog path plus local image id; moving the catalog changes those identities.
- Collections use catalog-local ids scoped to the exported library. Nested sets
  retain parent links, keyword trees retain synonyms, and smart albums contain
  a data-only `SavedSearch` AST. Lua is never evaluated.
- Current CRS edits use the engine-api `CrsKey` table and `Recipe::edit`, so every
  imported recipe validates against history replay. Curves normalize Adobe's
  0–255 coordinates. Supported mask geometry and AI mask kinds are translated.
  Unknown keys, unsupported structures/resources and invalid values are retained
  in `Recipe.unknown` with diagnostics, not silently discarded. Mask XML is
  retained for legacy flat groups and any partly understood payload. LR-4 removes
  the per-key source only for audited, completely consumed new parametric shapes;
  see [the LR-4 contract](../../docs/coordination/LR-4-PARAMETRIC-MASKS.md).
  PV1/2 imports carry a warning.
- Historical steps and snapshots are preserved as complete source rows on each
  image and in recipe extension fields. They are not falsely represented as
  replayable native edits. Faces retain region/cluster columns and keyword-face
  links, stacks retain source metadata and membership, and GPS is exposed as a
  latitude/longitude pair.

## Plan fields for hosts (M2-13b)

`ImportPlan::roots` keeps the `AgLibraryRootFolder` rows so a host can relocate
a moved drive (the app maps each root to its new location before writing).
`ImportedImage::{rating, pick, color_label}` keep the source selection columns
so the host can preview the Lightroom → Tessera selection mapping (docs/06
§2.1) and let the user rename or drop colour labels. Both are `serde(default)`,
so older serialized plans still load.

Imported smart-collection rating comparisons (`>=`, `>`, `=`, `<=`, `<`, `!=`)
and inclusive string ranges (`2..4`, with `=` or `between`) are translated to
unions of native decision/grade buckets rather than treating Lightroom stars
as Tessera grades. Zero stars / `unrated` maps to `decision:undecided`; one star
to Keep with no grade, two to grade 1, three/four to grade 2, five to grade 3.
Positive pick and reject flag rules become decision terms. Unsupported shapes
stay in the original AST and fail explicitly on compilation.

This mapping is lossy: three and four stars share grade 2, so a boundary
between them cannot match Lightroom exactly. A picked unrated image and a
one-star image both become Keep without grade, while a rejected image loses
its original grade. Native searches use the union of mapped buckets touched
by a rule; consult `ImportedImage.rating`/`pick` when exact source membership
is needed. See `tools/orchestrate/wp/M2-02c/FINDINGS.md`.

## Previews.lrdata (`previews`)

Lightroom's standard previews are read only for the fidelity comparison.
`PreviewIndex::open(catalog)` copies `<stem> Previews.lrdata/previews.db` to
temporary storage and opens the copy read-only (as for the catalog). Its
`ImageCacheEntry` table maps `imageId` (the catalog image id) to `uuid` and
`digest`; the pyramid is `<uuid[0]>/<uuid[0..4]>/<uuid>-<digest>.lrprev`.

An `.lrprev` file is a sequence of sections, each introduced by a header:

| Bytes | Field |
| --- | --- |
| 0–3 | magic `AgHg` |
| 4–5 | header length, u16 big-endian (32 in practice) |
| 6 | version (u8) |
| 7 | kind (u8) |
| 8–15 | payload length, u64 big-endian |
| 16–23 | padding length after the payload, u64 big-endian |
| 24–(header length) | NUL-padded ASCII name |

The payload follows the header, then the padding. The `header` section is
Lua-like text describing the levels; `level_1`, `level_2`, … each hold one
baseline JPEG, smallest first. `parse_lrprev` returns every section,
`jpeg_levels` the JPEG levels, and `PreviewIndex::jpeg(id, min_edge)` the
smallest level whose long edge reaches `min_edge` (else the largest).
`jpeg_icc_profile` reassembles an embedded ICC profile so the host can convert
the preview to sRGB before comparing. This layout follows long-standing
third-party extractors; it is verified here only against `write_lrprev`, not
against files written by Lightroom.

## Synthetic fixture (`--features fixture`)

`fixture::write(dir)` writes `Catalog/Fixture.lrcat`, `Catalog/Fixture
Previews.lrdata` and JPEG originals under `Photos/2026/{wedding,portraits}`.
The catalog records its root as `/Volumes/Old Drive/Photos/`, so the photos
must be found by relocating that root. It contains six photos (one original
missing on disk), one virtual copy, picks/rejects/stars 0–5, colour labels
(`Red`, `Client`, `Blue`), a keyword tree with a synonym and a duplicated name
(`Paris`), a collection set with a collection and a smart collection, a second
collection, an unsupported smart rule (`labelColor`), a stack, a face, GPS,
history steps and an unknown develop key. Its "Lightroom previews" are this
module's own approximation of the edits, not Adobe renders. The CLI exposes it
as `tessera import lrcat --make-fixture <dir>`.

## Retouch translation (LR-3)

The catalog adapter reads `RetouchAreas` / `RetouchInfo` from
`recipe.unknown["lrcat_develop_source"].properties` after the ordinary decoder.
Supported explicit-source heal/clone circles and simple `Mask/Paint` paths become
`settings.locals.retouch`. Legacy comma-separated `RetouchInfo` strings, Lua
spot tables, and XMP resources with explicit source coordinates are supported.
Circles and simple paint paths use width-normalized radii and source offsets.
Plain `Mask/Circle`, `Seed`, and `MaskDigest` are accepted as approximate geometry
or retained provenance; `CenterValue` and unknown semantics remain retained.

Both keys are **approximate**: Adobe healing, feather and orientation conventions
have not been verified with Adobe-rendered synthetic charts or public DNG+XMP
pairs. The exact source stays in `lrcat_develop_source`. A shared-format info
entry in `lrcat_translation_diagnostics` explains the approximation; successfully
mapped keys emit no warnings. Unsupported keys retain their source/diagnostics.
One `Author::Import` history entry contains the complete imported settings.

Registered Develop CPU and GPU-host paths execute retouch before Detail/Tone;
nonempty retouch requires schema 4 on serialization (base schema stays 3).
The caller-owned brush renderer must be supplied; standalone calls without it
fail explicitly. Camera-linear smart-preview admission still requires originals.

Coordinates use the current Develop input frame before common lens distortion
and crop: unrotated active pixels for CFA RAW, possibly already-oriented decoded
pixels for rendered RGB. File EXIF rotation follows RAW rendering. Healing uses
Tessera's Poisson solver with one union mask and immutable source per spot; no
Adobe solver parity is asserted. Synthetic regressions cover import/history,
source retention, render ordering, two independent spots, backend equality,
scaled rendering, and rotation/crop/lens-profile behavior. See
[LR-3 handoff](../../tools/orchestrate/wp/LR-3/HANDOFF.md) for unsupported sites and
remaining full-sensor loupe performance work.

Schema references: [ExifTool CRS tags](https://exiftool.org/TagNames/XMP.html#crs)
and [go-xmp CRS types](https://pkg.go.dev/github.com/mholt/go-xmp/models/crs).
They document shapes/names, not an Adobe pixel-equivalence specification.

## Verification boundary

There is no real catalog fixture. `tests/make_fixture.rs` creates a synthetic
SQLite catalog with Lightroom table and column names. Tests cover paths,
selections, virtual copies, hierarchy/synonyms, smart rules, curves/masks, GPS,
faces, stacks, history/snapshots, JSON round trips, additional columns, missing
core tables, and committed WAL reads with byte-for-byte source preservation.
Parser tests cover malformed/unsupported data and non-executable Lua parsing.
The rating-rule table test exercises every star threshold and comparison,
ranges, flags, and compilation against mapped synthetic-catalog selections.

Real Lightroom schema variants and render equivalence remain unverified.
Lens Blur controls are imported approximately into native focus/blur fields,
with exact source retention and per-field info reasons through the shared
`diagnostics::push_approximate` channel. `Active` is an exact boolean translation
and produces no approximation record. Inactive Lens Blur and standalone
DepthMapInfo preserve their old bytes and warnings without adding info records
or enabling an effect. Only native focus falloff, Adobe controls, or a depth
reference requires schema 4; plain native blur remains schema 3.

During import apply, `LrcatImport::apply_with_depth_resolver` passes opaque IDs
and the catalog image ID to the existing caller-owned resource association seam.
Only independently decodable grayscale PNG/TIFF with the pre-geometry dimensions
is accepted. Neither resource IDs nor Adobe helper tables are interpreted as paths.
`apply` without a resolver records an info-only `regenerated depth: no Adobe depth
resource; Tessera estimates depth at render` reason under DepthMapInfo (or LensBlur
when DepthMapInfo is absent). Successful resolution removes that pending reason.
No regeneration-completion diagnostic is emitted by rendering.

Resolved depth is stored before the imported recipe is published. Its key is a
stable digest of the image ID; ordinary user edits preserve the key. The on-disk
bound is one raster per imported image, at most **256 MiB including the 48-byte
header/checksum per raster**, under Tessera Application Support
`previews/depth-cache/pinned`. This is a steady-state bound; atomic replacement
uses at most one additional raster-sized temporary file per concurrent writer.
Re-import atomically replaces that image's slot;
re-import without usable depth removes its previous slot. `Engine::forget_missing`
removes the slot with the image record, and retains it when the image still exists.
This durable storage is separate from the 256 MiB evictable inference cache.
Rendering reads the imported key, downsamples it in memory for matching preview
pyramid levels, and never attaches resources to recipe history or writes
translation diagnostics. No import depth file is written to Lightroom storage.
Proprietary Adobe resource decoding and automatic catalog/resource association
remain unavailable; callers must supply independently resolved bytes through the
resolver callback. The default apply path honestly leaves regeneration pending.
DCP profiles, arbitrary retouch/Look payloads, and AI pixel blobs are not resolved
or rendered by this crate. Inspect diagnostics before persisting an import.

Run with `CARGO_TARGET_DIR` outside the repository:

    cargo test -p import-lrcat --release
    cargo clippy -p import-lrcat --all-targets --all-features -- -D warnings
    cargo fmt --check

## LR-7 Upright geometry

The catalog adapter translates the selected `UprightTransform_1..5` CSV matrix
into `settings.geometry.upright.homography`, a source-to-output 3×3 map in unit
image coordinates. `PerspectiveUpright` uses the existing codec numbering:
0 Off, 1 Auto, 2 Full, 3 Level, 4 Vertical, 5 Guided. The optional homography
is omitted when absent; no format version changes. CPU geometry uses its inverse
before manual transform/crop and after the inverse manual-transform step; saved
matrices bypass image line detection. The resident lens plan uses the same map.

Guided mode accepts `UprightFourSegmentsCount` (2–4) and a complete set of
`UprightFourSegments_0..3` CSV endpoint coordinates (x1,y1,x2,y2 in [0,1]).
Without a saved matrix, Tessera recomputes from the guides and reports that
Adobe solver parity is not guaranteed. Existing Perspective Vertical,
Horizontal, Rotate, Scale, Aspect, X and Y mappings are unchanged.

Only successfully translated keys leave
`recipe.unknown["lrcat_develop_source"].properties`. Inactive solutions, solver
version/digest/center/focal metadata, unsupported representations and invalid
matrices/guides remain exact Lua literals or XMP fragments. Matrix validation
rejects nonfinite, singular and source-frame pole-crossing maps. Adobe render
parity has not been measured; synthetic CPU tests check coordinate accuracy.

Enabled `EnableDistractionRemoval`, `GenerativeRemove` and `GenerativeFill`
produce a user-facing import report entry: "requires Adobe cloud; not
translatable", with an explanation that rendered Adobe pixels are needed and
a rendered TIFF export preserves their appearance. Source stays retained.

### LR-7c review correction

The earlier LR-7/LR-7b translation descriptions are superseded for saved Upright
and legacy CA: these are **approximate**, with exact source retained under
`lrcat_develop_source` and info-level `lrcat_translation_diagnostics` (`approximate: ...`),
not user-facing warnings. No Adobe-rendered/public DNG+XMP reference has verified
matrix direction/layout, center/focal frame, or CA sign/units. Center/focal
metadata now defines the assumed normalized frame documented in sidecar/UNMAPPED.md.
Saved solutions carry their mode and are cleared on mode/guide edits. Legacy CA
is gated to Adobe PV1/2 and zero values do not create fields or history. Shared
standalone sidecar import/export supports both families; all settings are recorded
in one import-authored history entry. Invalid matrices fail Recipe validation.

## LR-2e tone curves, monochrome, and legacy controls

Catalog Lua and XMP run the additive `lr2` pass once, after exact source capture.
LR-2 mutates settings directly. LR-7's shared `geometry::finish` records the one
replayable Import edit against the codec's `history.base`, after both lanes finish.
The Lua path skips both lane passes on its generated XMP packet, then applies
each once to the original Lua values after retention. Unrelated modern imports
and inactive B&W/extended-curve defaults preserve the original recipe bytes
and 2,000-row golden digest.

- On modern Adobe versions, nonidentity ExtendedToneCurvePV2012{,Red,Green,Blue}
  populate `/settings/tone/curves_extended` only with HDREditMode=1. Both axes
  divide by 255 without clipping signed/HDR knots. Extended-curve import leaves
  ordinary `/settings/tone/curves` intact; identity extended curves are
  provenance-only. Malformed curves
  stay retained with a warning. Extended point/channel curves compose **after the
  ordinary `curves.parametric` sliders** on CPU, GPU, and Adobe paths. An active
  extended block selects the point/channel rendition; it never replaces those
  parametric controls. Omitted extended channels are identity. All-identity HDR
  imports retain the ordinary point curves and the same parametric sliders.
- ConvertToGrayscale and GrayMixer* populate optional
  `/settings/color/monochrome {enabled,mixer}`. Disabled B&W with a zero mixer is
  a strict settings/history/hash no-op. A nonzero disabled mixer remains editable
  but has no pixel effect. Sidecar CRS read/write supports the same optional block.
  B&W conversion now precedes point/channel curves; grading and other colour
  controls follow, so channel-curve toning survives. CPU/GPU use the same mix.
- Adobe PV1/2 uses `/settings/tone/legacy_pv2010`, never PV2012 slider heuristics.
  The Adobe family check excludes native revision 2. Legacy values win when
  both spellings occur; stale modern tone sliders, Clarity2012, Texture, Dehaze,
  parametric sliders and PV2012 point curves (including extended curves) are
  cleared for this branch. They retain exact source and get `push_ignored`
  diagnostics with no recipe field. Even a legacy row without legacy sliders
  gets an explicit empty block. Saved Adobe PV1/PV2 recipes without the block
  fail rendering with "re-import needed"; re-import creates the supported block.
  HighlightRecovery precedes Recovery; Shadows precedes Blacks. Shadows=5 is
  stored as legacy blacks=5, not converted into modern blacks=-5. Brightness is
  a bounded rational operator; its largest relative lift is in deep shadows.
  Contrast currently pivots at linear 0.5. Both remain documented approximations.
- All active LR-2 mappings are `approximate`: the recipe contains numeric fields,
  exact source remains in `lrcat_develop_source`, and
  the shared `diagnostics::push_approximate` helper records an entry with
  `level:"info"`, `status:"approximate"`, `lane:"LR-2"`, the matrix recipe path,
  and the reason. Readers use `diagnostics::entries()`; the report shows a
  separate "Approximate translations" group.
  Approximation emits **zero user-facing warnings**. Public Adobe documentation
  supports the control meanings but does not establish calibrated render parity.
  See [the reference specification](../pipeline-cpu/LEGACY_PV2010.md).
- AutoToneDigest* is silently retained cache metadata. DepthMapInfo remains with
  LR-5/LR-6. Other unsupported controls retain their existing diagnostic behavior;
  the [matrix](../../docs/coordination/LR-TRANSLATION-MATRIX.md) gives dispositions.
- Native GPU legacy Tone uses the existing CPU-stage fallback. Legacy recipes
  decline resident/fused tone dispatch; Adobe compatibility stages remain CPU.
  Synthetic full GPU-session parity is tested alongside operator/batch parity.

Schema uses the shared `V4_FEATURE_PREDICATES` registry: enabled monochrome **or a
nonzero disabled mixer**, `curves_extended`, and `legacy_pv2010` require v4.
Each predicate uses `assert_bumped_only_when_present`; the lane-local schema
helper is gone. LR-7 owns the first-lane LR-SCHEMA checklist changes; LR-DIAG owns the
shared approximation guard. LR-2 adds only its feature predicates/tests and its
synthetic matrix input context (legacy process version or HDR mode).

Run `bash tools/orchestrate/wp/LR-2/gates-e.sh` from the workspace root for the
LR-2e synthetic gate. Earlier scripts and handoffs are historical evidence.

### LR-1 Point Color

`PointColors` now maps through the shared sidecar codec into
`/settings/color/point_colors`: Lua SDK tables (including contiguous explicit
array indices), equivalent RDF resources, and 19-number XMP swatch sequences.
The existing point shifts/range are reused; an optional `selection` stores
source HSL and all twelve feather boundaries. PointColors is `approximate`:
exact Lua/XMP source is retained, and the shared `diagnostics::push_approximate`
channel appends an info entry for `/settings/color/point_colors` (lane `LR-1`).
Partial, unknown, malformed and placeholder shapes keep their retention contract.
A nonempty point list requires schema v4 through the shared predicate registry.
LR-7's shared finish owns the single Import history entry for all lanes.

The import report labels rendering approximate. The SDK does not specify
Adobe's color-space/range/shift math, and this lane had no Adobe pixel oracle.
See [Point Color CPU reference](../pipeline-cpu/POINT_COLOR.md) for formulas,
source links, supported limits, preserved signed/HDR residuals, and the synthetic
reference tolerance. This is not a claim of Lightroom render parity.

When B&W is enabled, Point Color selects and adjusts colour before the B&W
conversion and tone curves; grading follows. Resident/fused GPU dispatch declines
point lists and the shared CPU fallback preserves this order.

### LR-9c cloud report

Visual edits requiring Adobe cloud use warning diagnostics with `status: cloud`.
They remain visible under **Requires Adobe cloud (not rendered)** in the import
sheet and saved Markdown report, with per-feature photo counts and examples.
Generative remove/fill and active distraction removal are never classified as
ignored. Mixed RetouchAreas can contain both an approximately rendered heal and
an unrendered generative item. Nonempty FilterList remains an unsupported warning.
The pre-import summary and plan preview have no cloud group, so the same
effects stay in their unsupported lists there. `ignored` is reserved for source values with no visual effect and is omitted
from the report. On the LR-5b stack, LR-5's regeneration note is reported as its
own approximate group beside these, on apply and on resume.

## LR-5b AI masks

**The app regenerates AI masks. It does not read Adobe mask rasters from
`.lrcat-data` or `.lrdata`.** Imported Subject, Sky, Background and prompted
Object descriptions use Tessera segmentation, with the same model path as native
AI masks (downloaded on first use).
No Adobe render parity is claimed. Person sub-parts (including Hair, Lips and
Teeth), People/person instances and unverified subtype/part IDs remain unsupported:
exact source is retained with a warning, and they are never broadened to Subject.
A nonzero `MaskSubCategoryID` is a part ID on any category, including masks named
`Mask/Subject`, `Mask/Sky` or `Mask/Background`, and is unsupported the same way.

The optional `apply_with_mask_resolver` / `apply_with_resolvers` bridge accepts
independently decoded, caller-associated grayscale PNG/TIFF rasters. This is an
injection interface, not an Adobe resource reader, and the app does not supply
one. Resource IDs are opaque, never filenames. An injected raster must have the
extent the renderer masks in, measured with the renderer's own recognizer and
decoder: RGB sources after EXIF orientation, RAW sources by active sensor area
(never a container preview or unrotated file dimensions). The source is only
measured for an AI-masked image when a mask resolver is supplied, through the
mask-specific guarded closure. The separate depth extent measurement runs only
when a depth resolver is supplied. Each apply attempts at most 256 resources;
invalid resources consume that attempt budget. The validated extent determines
u16 output cost before resolution, and no further resource is resolved once
another plane cannot fit the 256 MiB accepted-output budget.
Raster provenance fields (`FullMaskSize`, `LocalInputDigest`,
`LocalInputDigestVersion`) are accepted on AI mask kinds only. Retained person, part
and instance masks report "AI person, part or instance selection is not implemented".
Invalid or absent resources receive a push-only info diagnostic during apply:
`regenerated: no Adobe mask raster; Tessera re-segments at render`. This means
regeneration is requested, not that inference has completed. Successful resolution
never emits that note. Rendering never edits recipe history or import diagnostics.

In preview and interactive rendering, if any enabled AI component is pending,
failed or unavailable, its entire local adjustment has zero effect, including
inverted, subtractive and nested masks, and the mask UI shows pending/unavailable.
Export never produces an image that differs from what the user will see once the
mask exists: in file export, DNG and print, a model that cannot be loaded, a
backend that fails and an invalid raster are all errors, and nothing is published.
Missing, corrupt or wrong-extent stored rasters request regeneration with a
diagnostic; the regenerated plane renders in preview and export. Stored rasters
render without loading a model. MCP export rejects AI masks because its tool call
has no inference/cache inputs.

`MaskComponent.adobe_ai` is optional and round-trips through recipe JSON and native
XMP. It retains category, resource identity, regeneration state and a content hash.
Imported rasters use checksummed **u16** samples under the caller-owned app support
root. Preview, file export, print and Open Developed Image receive that explicit
root. A preview session
keeps loaded planes in memory by immutable content key, so ordinary frames do not
reopen raster files. Imports without AI masks make no mask-store calls.

Each apply accepts at most 256 resources and 256 MiB of stored rasters including
headers/checksums; decoding is separately bounded. Content keys include dimensions
and quantized samples, so reimports and interrupted publication cannot replace
pixels referenced by an earlier recipe. A per-image ownership record lists the
keys of the published recipe; during an apply it also holds the previous keys, and
a failed publication restores it. Import never deletes or lists the store, so a
raster superseded by a successful reimport becomes an orphan. Shared content
survives removal of one owner. Explicit `Engine::prune_missing` collects missing
owners and orphaned blobs; dry runs leave both untouched. Removing images drops
their records and collects once per batch; images that never owned a raster cause
no listing. Pin writes do not scan directories. A failed publication surfaces
its original error even if ownership rollback also fails. An image reimported
with no AI masks at all opens, creates and lists nothing in the mask store; once its
recipe is published, its own stale ownership record (if any) is unlinked by path,
leaving the content for explicit pruning. LR-6's shared depth-pin
functions and f32 depth representation remain unchanged.

All committed fixtures and mask pixels are synthetic. No Lightroom-managed storage
is written, no Adobe helper codec is claimed, and no import golden is re-pinned.
