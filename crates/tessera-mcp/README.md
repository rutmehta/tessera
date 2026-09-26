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

The ten `DocumentToolCall` tools (`open_document`, `add_layer`,
`set_layer_props`, `paint_stroke`, `set_pixel_selection`,
`apply_adjustment_layer`, `transform_layer`, `merge_down`, `export_document`,
`list_layers`) are listed with schemas derived by `build.rs` from engine-api's
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
selection crates can be linked without changing the executor. Built-ins:
`RoundBrush` (round dab, hardness falloff, spacing, flow build-up, opacity,
pressure size/flow) and `BasicSelection` (rectangle/ellipse marquee and
polygon lasso with 4×4 supersampled edges, box-blur feather). Layer
transparency, saved selections, inverse and replace/add/subtract/intersect
are handled by the executor.

Actions: `actions_record {name}` records every successful non-query call;
`actions_stop {path?}` returns (and writes) the `.tessera-action` JSON;
`actions_play {path | action, documents}` replays it. Documents, created
layers and saved selections are stored as `{"$input": i}`, `{"$doc": k}`,
`{"$layer": k}`, `{"$selection": k}` references, so an action replays on
documents with different layer ids and sizes; see `src/actions.rs` for the
format. `tessera actions play <file> <inputs…> --out-dir DIR [--format …]`
batches it from the CLI; `tessera actions show <file>` validates.

Known gaps: saved selections live outside history (undo keeps them);
`merge_down` needs a pixel layer below and keeps the lower layer's mask and
properties; `transform_layer` handles pixel layers (content and mask) and
smart objects only; PSD import warnings are not surfaced; image export ignores
`profile` (Unsupported) and embeds no ICC; there is no magic wand
(no `SelectionShape` exists for it).

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
