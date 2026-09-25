# engine-api contracts (v1.1.0)

`engine-api` is the one crate every other engine crate links against. It holds types, traits and the small amount of logic that makes them trustworthy (canonical hashing, history replay, colour-matrix algebra), and depends on nothing in the workspace. Any change to a public type or a serialized form bumps `CONTRACT_VERSION` in `src/lib.rs`, gets an Opus review, and is noted at the bottom of this file.

## Modules

**`tile`**: Images are cut into `TILE_SIZE` (256) square tiles at every pyramid level. Level 0 is full resolution and each further level halves the size, rounding up. `TileCoord { level, x, y }` addresses a tile. It sorts coarse levels first and then in raster order, which is the order progressive rendering needs. A `Tile` holds planar samples in one of four `TileFormat`s: `F32Planar`, `F16Planar`, `U16` or `U8`. Each tile has a halo of up to `MAX_HALO` pixels on every side. Tiles are reference-counted: `clone()` is O(1), and `samples_mut`/`plane_mut` copy the buffer only when it is shared. `Pyramid` is the read interface for anything tiled, such as decoded sources, cached stage outputs, masks and layer rasters. It has default methods for level count, level extents, edge-tile extents and bounds checks.

**`color`**: The raw pipeline's working space is `WorkingSpace::LinearRec2020` (the default). ProPhoto, P3, sRGB and ACEScg are also available for compatibility paths. `ColorMatrix3` is a row-major f64 3×3 matrix with `apply`, `inverse`, `*` and `rgb_to_xyz(primaries)`. It is f64 so that chained conversions keep their precision until they are baked into a kernel with `to_f32()`. `WhitePoint` holds xy chromaticity and constants for the standard whites. `ChromaticAdaptation` is CAT16 (default) or Bradford. `Illuminant` covers DCP calibration illuminants and maps to and from EXIF `LightSource` codes. `IccProfileHandle` is the BLAKE3 digest of the profile bytes, so it means the same thing on every machine. A registry in the colour crate resolves it to bytes and a CMM transform.

**`stage`**: `StageId` lists the 14 pipeline stages in their fixed order (spec 04 §3), from `Decode` to `Output`. Its discriminant is the stage's position. Every stage's parameter struct implements `StageParams`, whose `param_hash()` is a BLAKE3 digest of the struct's canonical JSON. `ParamHash::chain` folds a stage's hash with the hash of everything upstream of it. `MemoKey { image_id, stage, params_hash, tile }` is the key for memoized stage-output tiles. `params_hash` is always the *chained* hash for that stage. The pyramid level lives inside `tile`, so the key has no separate `level` field (see ambiguities below).

**`recipe`**: This is the per-image edit document, stored as `.edits/<image>.json`. `Recipe` contains:
- `schema_version`
- `image_id`
- `process_version`: Native revision N, or Adobe PV1–6 (converts to and from `crs:ProcessVersion`; see "Process version export policy" below)
- `settings: DevelopSettings`: one field per stage, in pipeline order
- `selection`
- `history`
- `ids`: monotonic mask and retouch counters
- `provenance`: informational source properties recorded by importers (not render-affecting)
- `unknown`: top-level members from newer writers, preserved on round trip

`recipe_hash()` covers `process_version` and `settings` only. It is the render and preview cache key. `stage_chain()` gives the per-stage memo hashes, seeded by the process version. `History` is append-only. Each entry stores JSON-pointer patches against the parent state, plus `parent`, author, label, timestamp, group and rationale. Undo and redo move `head`, and an edit made after an undo starts a new branch. Snapshots are named pointers to entries. `Selection { decision, grade, mark }` follows spec 06 §2: a grade is only valid on a Keep, and a mark is stored by name. `crs::CrsKey` lists every develop key named in spec 05 §3.2, each with its XMP namespace (`XmpNamespace::{Crs, Aux, Xmp}`), local name, Adobe type and range, and a `CrsTarget`: `Field(pointer)` for the recipe field it maps to, `Legacy`, or `Informational`. Camera and lens profiles are identified by `CameraProfileRef { name, digest }` and `LensProfileSource::Database { profile: LensProfileRef { name, filename, digest, setup } }`, so every Adobe identity field round-trips.

**`jobs`**: `Priority` has the classes `Ui < Viewport < Prefetch < Preview < Score < Export`. The derived `Ord` sorts the most urgent first. `CancellationToken` is a tree of flags: cancelling a token cancels its descendants but not its ancestors, and `check()?` returns `EngineError::Cancelled`. A `Job` is object-safe (`run(self: Box<Self>, &JobContext)`) and delivers its results through side effects it owns. `Scheduler` has `submit`, `reprioritize`, `cancel` and `status`, where the last three take a `JobTarget` (one job or a group).

**`tools`**: This is the typed tool API from spec 10 §2. `ToolCall` has one variant per tool: `set_tone`, `create_mask`, `adjust_mask`, `remove_object`, `retouch_skin`, `apply_style`, `crop`, `compare`, `get_histogram`, `get_scores`, `index_folder`, `set_selection` and `export`. It is internally tagged on `"tool"`, so each variant name is the MCP tool name and each flat JSON object is one call. `ToolRequest` wraps a call with `rationale`, `group` and an optional `expect_recipe` hash for optimistic concurrency. `ToolOutput` holds the results, and `ToolResponse` is `{"ok": …}` or `{"error": EngineError}`.

**`error`**: `EngineError` is the only error type allowed across a crate boundary. It is `Clone` and serializable as `{"code": "<snake_case>", …}`, so it can be cached with a failed job and returned verbatim to MCP clients. `EngineResult<T>` is the alias.

**`id`**: These are opaque newtypes for identifiers:
- `ImageId`: 128 bits, 32 hex characters, stored in the sidecar
- `MaskId`, `RetouchId`, `HistoryEntryId`, `HistoryGroupId`, `PersonId`, `JobId` and similar
- string ids such as `ProfileId` and `StyleId`
- `ModelRef { id, version }`, which every ML-derived result records
- `Digest`, a 256-bit BLAKE3 digest serialized as hex

## Invariants other crates must keep

1. **Stage order is fixed.** Never reorder `StageId` or insert a stage in the middle without bumping the native process revision. Memo keys and history paths depend on the order.
2. **Hash inputs are deterministic.** Anything that implements `StageParams` or is reachable from `DevelopSettings` must serialize deterministically. That rules out `HashMap`, timestamps, caches and interior mutability. Use `Vec` or `BTreeMap`. The canonical form sorts object keys and folds `-0.0` to `0`, so the declaration order of fields does not matter.
3. **The recipe hash covers exactly the render state.** Selection, history, snapshots, id counters, `provenance` and `unknown` never feed `recipe_hash()` or `stage_chain()`. Anything that changes pixels must live in `settings` or `process_version`.
4. **A change in rendering requires a new process revision.** If an operator renders the same parameters differently, bump `ProcessVersion::NATIVE_CURRENT.revision`. Existing recipes then keep their old revision, and caches cannot mix the two.
5. **`settings == history.state_at(history.head)`.** Mutate settings only through `Recipe::edit`, `checkout`, `undo`, `redo` or `restore_snapshot`. Never write `recipe.settings` directly outside a migration. `Recipe::validate()` checks this invariant.
6. **History is append-only.** Entries are never edited or removed. `HistoryEntry.id` equals its 1-based position, and a parent always precedes its child. Mask and retouch ids are allocated with `allocate_*_id` and are never reused, even after undo.
7. **Schema evolution:**
   - Every struct is `#[serde(default)]`, and a missing field means "neutral".
   - A new field always needs a neutral default.
   - Renaming or removing a field is a breaking change: it breaks the paths stored in history, so it needs a migration and a new `RECIPE_SCHEMA_VERSION`.
   - Any additive schema change also bumps `RECIPE_SCHEMA_VERSION`, because older builds open newer documents read-only (`to_json` refuses to write them) and would otherwise drop fields silently.
   - The golden-hash test in `recipe/mod.rs` fails whenever the default serialized state changes. When that happens, update it on purpose, because it invalidates every render cache.
8. **Selection:** `grade.is_some()` ⇒ `decision == Keep`. Use `set_decision` or `set_grade`, and call `normalized()` on external input. AI signals never change a `Decision`.
9. **Tiles:**
   - In-flight pipeline math is `F32Planar`.
   - `F16Planar` is allowed only for cached stage outputs (execution plan §1.4).
   - A tile's interior is at most `TILE_SIZE`, and edge tiles are clipped with `Pyramid::tile_extent`.
   - Halos are at most `MAX_HALO`.
   - Never hold `samples_mut` on a tile shared with a cache unless you intend to copy it.
10. **Jobs:**
    - Poll `ctx.check_cancelled()` at least once per tile or per equivalent unit of work, and return `EngineError::Cancelled` promptly.
    - A job must not block on a job of a lower priority.
    - Schedulers never start a queued job that has been cancelled, and never make a more urgent class wait behind a less urgent queued job.
11. **Tools:**
    - A mutating tool call becomes exactly one history entry. It records the request's `rationale` and `group` and has `Author::Agent`.
    - Tool names are the snake_case variant names and are stable once shipped.
    - No tool produces generative pixels.
12. **Errors:** Convert crate-private errors into `EngineError` at the boundary. `Internal` is reserved for invariant violations, meaning bugs.
13. **`crs:` table:**
    - Each `CrsTarget::Field` pointer must resolve in a default serialized `Recipe`. A test enforces this.
    - Match properties by `key.namespace().uri()`, never by prefix. Local names are unique across namespaces, and `CrsKey` serializes as the bare local name; `Display` and `qualified_name()` give `prefix:Name`.
    - `Legacy` keys (PV1/2 manual CA `ChromaticAberrationR/B`): the importer flags them and converts them best-effort.
    - `Informational` keys (all `aux:Enhance*`) describe pixels Adobe already baked into the file. The importer records their raw values in `Recipe::provenance.properties` under `qualified_name()` and never turns them into an edit (for example, it must not enable neural denoise). Exporters never write them from a recipe but must preserve them in a source packet.
    - The lens-profile keys `LensProfileEnable/Setup/Name/Filename/Digest` jointly encode one `LensProfileSource`: `Enable = 0` gives `None`; any non-empty name, filename or digest gives `Database` with all four identity fields; otherwise `Auto`.
14. **Process version export policy:**
    - `ProcessVersion::crs_value()` returns a `CrsProcessVersion { process_version, native_revision }`.
    - Adobe PV1–6 export as their own string with no companion. Any other Adobe revision returns `None`, and the exporter must refuse.
    - Native recipes export as **best-effort PV6**: `crs:ProcessVersion = "15.4"` plus `ts:NativeRevision = <revision>` (`crs::TS_NAMESPACE`, `crs::NATIVE_REVISION_PROPERTY`). Adobe software renders those settings with its own PV6 math, so the result is close but not pixel-identical. Say so wherever the user exports for Lightroom.
    - Import uses `ProcessVersion::from_xmp(crs, companion)`. The companion is honoured only next to `"15.4"` and only as a positive integer; otherwise the document is the Adobe process its `crs:` value names.
    - Exporters write or remove the companion together with `crs:ProcessVersion`, so it never goes stale.
    - Known limitation: if Adobe software edits a best-effort-PV6 file and keeps our foreign `ts:` property, we still read it back as native.
15. **Selection XMP mapping** (established by the sidecar crate, M1-03):

    | Selection | `xmp:Rating` | `xmpDM:pick` | `xmpDM:good` |
    | --- | --- | --- | --- |
    | Undecided | absent | absent | absent |
    | Reject | `-1` | `-1` | `False` |
    | Keep | `1` | `1` | `True` |
    | Keep, grade 1 / 2 / 3 | `2` / `3` / `5` | `1` | `True` |

    - `xmpDM:` is `http://ns.adobe.com/xmp/1.0/DynamicMedia/`. `pick` is an integer (1 picked, 0 unflagged, −1 rejected) and `good` is a Boolean, as ExifTool's XMP2.pl `xmpDM` table defines them. Lightroom Classic writes flags to XMP from 13.2.
    - On read, `rating < 0` or `pick = -1` means Reject. Otherwise `pick = 1` or a rating of 1–5 means Keep, with ratings 2 / 3–4 / 5 giving grades 1 / 2 / 3.
    - A mark is written as `xmp:Label` text through a `MarkPreset` (Lightroom colour names by default). `ts:Mark` and `ts:MarkLabel` keep the exact mark name. The name is honoured only while `ts:MarkLabel` still equals the visible `xmp:Label`.
    - No `lr:` reject flag exists. Never write flag literals into `lr:hierarchicalSubject`.

## Spec ambiguities resolved here

- **"Recipe" names the whole document.** Spec 05 calls the `.edits` JSON (settings, history and snapshots) "the recipe". So `Recipe` is the document, `DevelopSettings` is the ordered stage parameters, and `recipe_hash()` covers `process_version` plus `settings`.
- **`MemoKey` has no separate `level` field.** Spec 04 lists `(imageId, stageId, hash, tileCoord, level)`, but `TileCoord` already carries `level`. A separate field could disagree with it, so it is exposed as `MemoKey::level()` instead.
- **History stores patches, not full states.** Entries hold JSON-pointer patches against the parent state instead of full copies, which keeps them small and makes per-step toggles possible later (spec 10). `History.base` is stored explicitly so that replay does not depend on today's defaults.
- **Marks are stored by name, not by index into the library's mark set.** A sidecar copied to another library then keeps its meaning, and the name matches the XMP `Label` text.
- **Adobe PV strings are assumed.** The Adobe process-version strings are taken as PV1 = 5.0, PV2 = 5.7, PV3 = 6.7, PV4 = 10.0, PV5 = 11.0, PV6 = 15.4.
- **Key names were verified against ExifTool (1.1).** In 1.1, every table name was checked against the `crs` and `aux` tables in ExifTool's `lib/Image/ExifTool/XMP.pm`. The `crs` table is in `XMP.pm`, not `XMP2.pl`, which holds `xmpDM`. The M1-03 findings were also used. What that settled:
  - `Enhance*` properties are `aux:` (`http://ns.adobe.com/exif/1.0/aux/`).
  - The luma amount is spelled `EnhanceDenoiseLumaAmount`.
  - `HDREditMode` is an integer and `HDRMaxValue` is a real.
  - `LensProfileSetup` is a string.
  - `PostCropVignetteStyle` is an integer where 1 = Highlight Priority, 2 = Color Priority and 3 = Paint Overlay.
  - `Dehaze` is a real.
  - All other names match ExifTool.
- **Still unverified (no Adobe schema, no local Lightroom run):**
  - `HDREditMode`: only value 0 has been observed. 1 = HDR on is assumed.
  - `HDRMaxValue`: its unit is assumed to be stops of headroom, and the 0–16 range is ours.
  - `LensProfileSetup`: the closed set `LensDefaults | Auto | Custom`. Auto and Custom have been observed.
  - The types of `EnhanceDenoiseVersion`, `EnhanceDenoiseLumaAmount` and `EnhanceSuperResolutionScale`, which ExifTool itself marks as uncertain. They only feed provenance, as raw text.
- **Lens distortion runs in `Geometry`.** Its parameters live in `LensSettings`, but spec 07 §3 has distortion applied in the composed geometry map. Only CA, vignetting and softness run in `Lens`.
- **Units follow the user-facing controls.** Exposure is in EV, sliders run −100..100, geometry is normalised 0..1, and hues are in degrees. The one exception is the Adobe defringe hue range, which uses 0–100 units in XMP and is converted by the importer.

## Change log

- M2-04b: native process revision 2. Custom white balance uses perpendicular
  CIE 1960 Duv; camera calibration retains the XYZ-to-camera inverse without
  independent XYZ row scaling. Default sharpening 40/1/25/0 and colour NR
  25/50/50 now render.
  No type or recipe schema changes. The default recipe hash is now
  `b053649aeb073ef9c9f3bd92d653c9cec26c5a2b5b9037e3399988355209fd9e`.
  Process-version-seeded stage and preview keys separate revision 1 from 2.

- 1.0.0 (M0-03): initial contracts.
- 1.1.0 (M2-03), recipe schema 2, golden recipe hash changed (`camera_profile.profile` default now serializes as `{"name":"","digest":""}`), all render caches invalidated:
  - `CrsKey` table: added `namespace()` (`XmpNamespace::{Crs, Aux, Xmp}`) and `target()` (`CrsTarget::{Field, Legacy, Informational}`). `recipe_path()` is now derived from `target()`. Added `is_informational()`, `qualified_name()` and `from_xmp(uri, name)`. `Display` and `FromStr` now use the key's own prefix.
  - `Enhance*` keys moved to `aux:` and made `Informational`, with no recipe path. `EnhanceDenoiseLumAmount` was renamed to `EnhanceDenoiseLumaAmount`. Added the `aux:` keys `EnhanceDetailsVersion`, `EnhanceSuperResolutionVersion` and `EnhanceSuperResolutionScale`. `EnhanceSuperResolutionAlreadyApplied` changed from legacy to informational.
  - `Dehaze` is now `Real(-100, 100)`, per ExifTool. `HDREditMode`, `HDRMaxValue`, `LensProfileSetup` and `PostCropVignetteStyle` were re-verified and left unchanged.
  - `CameraProfile` now maps to `/settings/camera_profile/profile/name` and `CameraProfileDigest` to `.../profile/digest`.
  - New namespace constants: `AUX_NAMESPACE`, `XMP_NAMESPACE`, `XMP_DM_NAMESPACE`, `TS_NAMESPACE` and `NATIVE_REVISION_PROPERTY`.
  - `CameraProfileSettings.profile` changed from `ProfileId` to `CameraProfileRef { name, digest }`. `LensProfileSource::Database.profile` changed from `LensProfileId` to `LensProfileRef { name, filename, digest, setup: LensProfileSetup }`. Both still deserialize from schema-1 strings, which become `name`, so old documents and history patches replay.
  - `ProcessVersion::crs_value()` now returns `Option<CrsProcessVersion>`, and native recipes export as best-effort PV6 plus `ts:NativeRevision`. Added `ProcessVersion::from_xmp` and `NATIVE_EXPORT_ADOBE_PV`.
  - Added `Recipe::provenance: Provenance { properties }`, an additive field that does not feed the recipe hash.
  - `Recipe::from_json` now upgrades older `schema_version`s to the current one after migration.
  - `CONTRACTS.md` now records the selection XMP mapping (invariant 15) and the process export policy (invariant 14).
