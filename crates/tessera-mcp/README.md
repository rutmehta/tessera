# Tessera MCP and synchronous console

`tessera-mcp --app-dir /path/to/Tessera` serves MCP JSON-RPC on stdio.
`tessera mcp --app-dir /path/to/Tessera` execs a sibling `tessera-mcp`, then
falls back to PATH. `TESSERA_MCP_BIN` overrides executable discovery. Diagnostics
go to stderr. No UniFFI layer or pixel generation is involved.

The official `rmcp` SDK is pinned to 3.4.1 and licensed **Apache-2.0** (its package
manifest inherits the [SDK workspace license](https://github.com/modelcontextprotocol/rust-sdk/blob/main/Cargo.toml)).
The server negotiates MCP 2025-03-26 and supports initialize, tools/list,
tools/call, resources/list, and resources/read.

## Tools and console

All thirteen engine tool names are exposed: `set_tone`, `create_mask`,
`adjust_mask`, `remove_object`, `retouch_skin`, `apply_style`, `crop`, `compare`,
`get_histogram`, `get_scores`, `index_folder`, `set_selection`, and `export`.
Additional tools are `open_image`, `render_preview`, `list_images`, and
`describe_image`. Engine tool arguments are the flattened ToolRequest object,
without its `tool` member. Supply `rationale`, `group`, and optionally
`expect_recipe` alongside operation parameters.

```rust,ignore
let mut console = tessera_mcp::Console::open(app_dir)?;
let image = console.open_image(photo_path)?;
let response = console.execute(engine_api::tools::ToolRequest {
    call: engine_api::tools::ToolCall::SetTone {
        image,
        update: engine_api::tools::ToneUpdate {
            exposure: Some(0.5),
            ..Default::default()
        },
    },
    rationale: Some("Lift the subject".into()),
    group: None,
    expect_recipe: None,
});
```

Console execution is synchronous; the MCP adapter runs it on a blocking worker
and serializes access. Indexing/export finish before responding; their engine
output variants still carry bookkeeping IDs. Helpers expose the four additional
operations and `compare_images` to in-process callers.

Every successful mutation records one Agent entry per distinct affected image,
including no-ops. Recipe changes use the engine's ordinary diff/history system.
Selection, indexing, and export record audit entries with empty develop patches;
the limitations of undoing these operations are described below. Unsupported or
invalid operations do not create successful history entries. Sidecar writes use
the existing atomic writer and update its vector clock.

Schemas are derived with schemars. `build.rs` reads the unchanged engine serde
declarations, creates schema-only mirrors, and adds the ToolRequest envelope to
each variant. Custom serialized ID types map to their string/integer wire types;
the real engine deserializer remains authoritative. Wrong shapes return JSON-RPC
`-32602`; valid operations that fail return MCP `isError: true` with EngineError.

`render_preview` accepts `image` and `max_px` (default 1024, range 1–4096), returns
PNG image content, preserves aspect ratio, and never upscales. `compare` returns
two PNG content blocks plus metrics for each image: normalized display luminance
mean, population standard deviation (contrast), and fractions clipped in any
RGB channel. DeltaE2000 compares matched normalized coordinates at a common
preview size; RecipeDiff counts engine parameter patches; Scores compares focus.

Display histograms count the full-resolution SDR sRGB output. The default 256-bin
path uses a resident GPU reduction, with CPU merging of integer band counts.
Other bin counts use a full-resolution CPU measurement, not a preview. For 256
bins, display luma is floor((2126 R + 7152 G + 722 B) * 256 / 2550000), capped
at 255. Clipping counts pixels with any encoded channel at 0 or 255. Scene-linear
histograms still use the <=1024px scene-linear Rec.2020 preview before display
mapping; they are not used by the objective critic. Both support 2–4096 bins
(default 256). No AI measurements change selection state.

In-process `Console::output_metrics` returns native-resolution clipping counts,
RGB/display-luma and linear-luma histograms, and an exact linear-sRGB mean from
the RGB marginal counts. `render_face_crop` returns native-resolution pixels for
an oriented normalized region; `render_noise_patch` returns a small central native
patch. CPU-only/RGB/unsupported recipes fall back at full resolution. The VLM
preview remains independent, and engine-api/tool schemas are unchanged.

## Layered documents (spec 02) and Actions

The fifteen `DocumentToolCall` tools (`open_document`, `add_layer`,
`set_layer_props`, `paint_stroke`, `set_pixel_selection`,
`apply_adjustment_layer`, `transform_layer`, `merge_down`, `export_document`,
`list_layers`, `add_channel`, `delete_channel`, `rename_channel`, `edit_channel`,
`load_channel_as_selection`) are listed with schemas derived by `build.rs` from engine-api's
serde declarations plus the `DocumentToolRequest` envelope (`rationale`,
`group`, `expect_head`). Extra tools: `describe_document`,
`render_document_preview`, `actions_record`, `actions_stop`, `actions_play`
(underscores, not `actions/…`, so names stay valid for clients that restrict
tool names to `[A-Za-z0-9_-]`). Tool names are unique across all enums.

`documents::Documents` keys open documents by session `DocumentId`. Each
session pairs the compositor `Document` (copy-on-write states) with an
engine-api `DocumentHistory`, one compositor state per entry. Every successful
editing call applies exactly one compositor op and records exactly one entry
with `Author::Agent`, the request's rationale and group, and
`Action::from_document_tool(call)`; no-op updates still record their entry.
Failures (including a stale `expect_head`, which returns `conflict`) change
nothing. `open_document` reads `.tessera-doc`, PSD/PSB and JPEG/PNG (one
Background layer; 16-bit PNG opens as 16-bit). `export_document` writes
`.tessera-doc`, PSD/PSB (with retained source records) or a flattened
JPEG/PNG 8/16/TIFF 8/16 rendered by the CPU reference compositor. Edits over
MCP also return a 512 px preview; previews come from the per-document
GPU-resident renderer at the finest pyramid level that fits (CPU fallback
without Metal or with `TESSERA_NO_GPU`).

Brushes and selections are pluggable (`BrushEngine`, `SelectionEngine`;
`Documents::set_brush_engine`/`set_selection_engine`) so the brush and
selection crates are linked by default. The brush adapter commits the real
brush engine's tile deltas directly (seed 0 for engine-api strokes), avoiding
double compositing. The selection adapter uses selection-crate geometry and
Gaussian feathering. Legacy engines remain available:
`RoundBrush` (round dab, hardness falloff, spacing, flow build-up, opacity,
pressure size/flow) and `BasicSelection` (rectangle/ellipse marquee and
polygon lasso with 4×4 supersampled edges, box-blur feather). Layer
transparency, saved selections, inverse and replace/add/subtract/intersect
are handled by the executor.

Actions: `actions_record {name}` records every successful non-query call;
`actions_stop {path?}` returns (and writes) the `.tessera-action` JSON;
`actions_play {path | action, documents}` replays it. Documents, created
layers and saved selections are stored as `{"$input": i}`, `{"$doc": k}`,
`{"$layer": k}`, `{"$selection": k}`, `{"$channel": k}` references, so an action replays on
documents with different layer ids and sizes; see `src/actions.rs` for the
format. `tessera actions play <file> <inputs…> --out-dir DIR [--format …]`
batches it from the CLI; `tessera actions show <file>` validates.

Saved selections participate in branching undo/redo. MCP `undo` and `redo`
accept `{document, expect_head?}`. `import_brushes {path}` reads ABR and persists
tips under `<app-dir>/brush-presets.json`; `list_brushes {}` returns full presets.
`paint_preset {document, layer, preset_id, points, target?, seed?}` resolves a
complete brush snapshot, preserving tip, dynamics, texture, symmetry and
clone/heal settings. The Rust `BrushPresetStore::save` API creates custom presets.

`select_advanced {document, operation: {kind, ...}, mode?, feather?}` exposes
`wand`, `quick`, `colour_range`, `object`, `subject`, and `sky`. The last three
require a real provider installed with `Documents::set_segment_model`; absent
providers return explicit errors. `refine_edge` exposes edge refinement and
`selection_boolean` combines the current mask with a saved selection ID.
Schemas in `tools/list` describe all parameters. Local edits accept rationale
and expect_head and retain one Agent history entry. Portable Actions do not yet
support these local commands, so calls during Action recording are rejected.

PSD import warnings are included beside `ok` in MCP open responses. Document
JPEG, PNG 8/16 and TIFF 8/16 exports embed ICC via color-mgmt (retained RGB
document profile or sRGB for untagged documents). Custom output profile handles
still return Unsupported. See `tools/orchestrate/wp/M5-10/NEEDS.md` for contract
requests; engine-api is unchanged.

Known gaps: `merge_down` needs a pixel layer below and keeps the lower layer's mask and
properties; `transform_layer` handles pixel layers (content and mask) and
smart objects only. Saved selections are persistent alpha channels; their
SelectionId is the alpha ChannelId's numeric value.

### Channel staging (engine-api 1.3)

`describe_document.summary.channels` and open-document summaries expose ordered
alpha/spot metadata independently of active selection. Channel edits retain the
normal concurrency/history envelope; IDs are not reused after undo or deletion
within a session. Spots are masks with preview metadata, not spectral RGB inks.

`stage_channel_raster {document, channel?}` snapshots the active selection, or
the supplied alpha/spot channel, into a host-owned immutable raster store. It
returns `{ok: {digest, depth, extent}}` for the `raster` argument of `add_channel`
or `edit_channel`. It does not edit history. A client can first construct a mask
with `set_pixel_selection`, stage it, then create an alpha or spot channel. A
missing selection/channel fails rather than silently staging an empty mask.
In-process hosts can also use `Documents::stage_channel_raster(Raster)`.

The digest is domain-separated BLAKE3 over extent, depth, and normalized f32
samples in row-major order (negative zero canonicalized). Staging checks one
plane, positive dimensions, finite samples in [0,1], at most 16,777,216 pixels
per raster and 67,108,864 retained pixels per Documents instance. Equal content
deduplicates. Add/edit validate the full reference against the staged data and
canvas. Handles survive undo and document close, but not process exit. Action
replay in a new process requires restaging identical content; action files do
not embed pixels. The store is released with Documents, not evicted mid-replay.

### People tools (engine-api 1.3)

`assign_person`, `confirm_person`, `merge_people`, `split_person`, and
`name_person` accept the LibraryToolRequest fields, including optional rationale.
`Console::run_library` returns a JSON result with the surviving/allocated
`person_id` and operation details. Calls are recorded and played as library
Actions, not recipe or document history entries. Faces must be nonempty, unique,
current `(image_id, ordinal)` references. Confirm and split validate membership
before edits; merge preserves target name and confirmation flags; split creates
an unnamed identity with reset confirmations. Manual assignment does not require
an embedding or quality eligibility. No automatic clustering job is launched.

`tessera://people` (or `Console::people`) exposes PersonSummary rows. The index's
opaque identities, including those produced by ml_faces, are adapted through
`<app-dir>/people-ids.json`. Existing canonical numeric IDs are retained when
available; other IDs receive monotonically allocated numeric bindings. Retired
IDs survive merges, and recreated model keys receive fresh bindings. Keep this
file with the catalog. Hosts must serialize access to the app directory and
perform identity deletion through this adapter (an externally deleted/recreated
key without an observed absence cannot be distinguished by the current index).
The index does not persist clustering approximation provenance: nonempty
membership is conservatively reported approximate, regardless of confirmation.

`writes.write_sidecars` and `writes.person_keywords` default false. Sidecar
export uses `cull::people::name_person`, normalized MWG face geometry and indexed
analysis-preview dimensions. Person keywords are additive accepted catalog tags;
XMP keyword writes require both flags. Clearing/renaming never removes old tags.
No sidecar reads, probes or writes occur with write_sidecars=false.

References and required analysis dimensions are checked before membership edits.
Merge/split use index transactions; assign/confirm apply per-face transactions.
Metadata synchronization follows membership changes and is not an atomic
transaction with them. Errors explicitly say when the catalog edit/name already
applied; earlier faces can remain applied on a database I/O failure. Naming uses
cull's preflight and ordinary-failure compensation for its name/library/XMP
changes, but catalog keyword acceptance is a subsequent operation. Do not retry
a failed split blindly. Only successful calls enter an Action recording.

## Preset lookup

`apply_style` resolves the StyleId through `<app-dir>/styles/index.json`:

```json
{"warm_portrait": "warm-portrait.json"}
```

The referenced JSON is a partial DevelopSettings object, or an object with a
`settings` member. For example:

```json
{"tone": {"exposure": 0.5}, "color": {"vibrance": 12.0}}
```

Only mentioned fields change. Amount 100 applies those fields exactly; numeric
float fields interpolate from current values at 0–200%. Changing discrete,
array, or string fields at fractional amounts returns Unsupported. Unknown
engine fields are rejected, and the resulting settings must render before the
sidecar is written. Mapped files must resolve inside `styles`, including after
symlink resolution. Style IDs are lookup keys, never paths supplied by clients.

## Resources

- `tessera://images`: catalog image JSON.
- `tessera://albums`: the library document's manual albums.
- `tessera://people`: persistent numeric identity summaries (reserves new ID mappings).
- `tessera://albums/<numeric-id>`: one album and its members.
- `tessera://images/<32-hex-id>/render`: low-resolution PNG blob.
- `tessera://images/<32-hex-id>/histogram`: 256-column RGB histogram PNG blob.

MCP resources use standard base64 blob content with `image/png`; image tools use
standard image content blocks. No custom JSON-RPC framing is introduced.

## Engine gaps (engine-api remains unchanged)

- `ToolOutput::Comparison` has no image or structured metrics fields. MCP adds
  both images and metrics; Console's typed response carries metrics in `summary`,
  and `compare_images` provides typed image/metric access.
- The four convenience tools have no ToolCall/ToolOutput variants; Console
  exposes methods for them without changing the engine contract.
- History patches target DevelopSettings only. Selection and catalog/export
  actions can be audited but cannot be undone through Recipe::undo. Supporting
  undo needs a broader history payload in engine-api. Batch history is per image;
  filesystem failures can leave an already-completed prefix of a batch.
- Index::scan is recursive only. `recursive: false` returns Unsupported, and the
  scanner's format allowlist excludes PNG. `open_image` indexes its parent folder
  using this API; JPEG, TIFF, and the scanner's RAW formats are supported.
- Procedural masks are supported. AI mask components need runtime raster/depth
  providers; the current reference-renderer path cannot evaluate those components
  from ToolCall alone, so they return Unsupported without changing the recipe.
- `remove_object` and `retouch_skin` explicitly return Unsupported until the
  non-generative implementations exist.
- Export supports sRGB SDR JPEG (quality 1–100), PNG 8-bit, TIFF 8/16-bit, long-edge
  and within-box resizing. Custom ICC handle resolution, HDR, PNG 16-bit, TIFF
  32-bit, JPEG XL, AVIF, DNG, and megapixel resize return Unsupported rather than
  silently substituting formats/settings.
- Scores cannot distinguish unknown from zero for eyes-open/aesthetics and do
  not carry signal provenance. `describe_image` includes stored signals with
  models and nullable eyes-open proxies, and explicitly labels its embedding
  caption as an unimplemented placeholder. It does not invent a scene caption.
- Engine indexing/export output names imply queued jobs, but provide no
  synchronous completion fields or exported paths. This Console completes those
  operations synchronously before returning their bookkeeping IDs.

## Verification

```sh
export CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-08
cargo test -p tessera-mcp -p tessera-cli --release
cargo clippy -p tessera-mcp -p tessera-cli --all-targets -- -D warnings
cargo fmt --check
```

Tests cover persistent Agent history and envelopes, rejected mutations, masks,
crop, style lookup/amount, selection, histogram bins, comparison images/metrics,
export pixels, CLI exec, in-memory MCP initialization/list/call/resources, invalid
schema errors, and a spawned stdio server.
