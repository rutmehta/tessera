# M5-15 implementation

## Model and history

`DocState.channels` stores ordered `DocumentChannel` records with `ChannelId`, name,
`ChannelKind::Alpha` or `Spot { color: [f32; 3], solidity: f32 }`, and a canvas-sized
single-channel COW Raster. `next_channel_id` is separate from layer IDs. Names may
repeat, matching PSD, so callers address channels by ID. Add with ID zero to allocate.

`DocOp::{AddChannel, DeleteChannel, RenameChannel, EditChannel}` participate in
normal atomic batches, undo, redo, and checkout. Raster shape and normalized finite
spot metadata are validated. History memory accounting includes channel tiles.
Native storage uses existing deduplicated zstd tile chunks, preserves channel depth,
and defaults absent channel fields for older manifests.

Spot rendering is intentionally a placeholder: stored colour and solidity do not
affect RGB compositing. Tests assert unchanged composite pixels. No GPU/resident
code was changed.

## Selection and MCP

`selection::channels::{save, load}` save lossless F32 masks into document history and
load by ChannelId. The old standalone `AlphaChannels` remains a compatibility utility
for detached masks, not MCP or document persistence.

MCP `save_as` now batches AddChannel with SetSelection into one history entry.
Saved selection lists, reload, and boolean operands derive from document alpha channels.
The previous independent saved/saved_nodes/next_selection storage was removed.
Opening native files exposes their saved channels, including at the opened history root.
`DocumentSession::saved_selections()` now returns an owned Vec rather than a borrowed slice.

## PSD

Extra merged-image planes are imported/exported separately from RGB and merged
transparency. The signed layer-count transparency flag determines the plane offset,
not simply the presence of a fourth plane. Metadata is regenerated when channels
change, including deletion, without replacing unrelated resources.

Names use standard image resources 1045 (UTF-16) and 1006 (MacRoman), not the layer
`unam` tag. Spot display information uses resources 1077 and 1007. Tests cover
byte-built external fixtures, malformed resources, PSD/PSB, supported compression
modes and 8/16/32-bit depth. Native empty documents get a transparent placeholder
layer in PSD so the merged-transparency flag survives serialization.

Limits: spot display colours currently require RGB; non-RGB spot display spaces
return an explicit error. PSD export quantizes samples to the document depth,
RGB display colours to u16 and solidity to integer percentages. Alpha overlay
colour/polarity is not part of the native model. No spectral spot preview is implemented.

## engine-api follow-up (unchanged here)

Needed contract additions: ChannelId; ordered channel summaries with id/name/kind,
spot display RGB/solidity, raster depth and extent; add/delete/rename/edit channel
calls; channel raster payload or tile-delta target; explicit load-channel-selection
support (including spot masks). Existing SelectionId maps to ChannelId's numeric
value for alpha channels. Document summaries should expose channels independently
of the transient active selection. Channel commands should carry the existing
expect_head, rationale and history grouping envelope.

## Verification

Executed the exact requested command with
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-15:

    cargo test -p compositor -p psd -p selection -p tessera-mcp --release && cargo clippy -p compositor -p psd -p selection -p tessera-mcp --all-targets -- -D warnings && cargo fmt --check

Exit 0. 218 tests passed, 0 failed, 7 existing benchmark tests ignored.
Clippy and formatting passed. Existing vendored LibRaw C++ warnings remain in the
build output. Complete output: `verification.log` beside this file.

`git diff --check` passed. All modified/untracked paths were checked against the
work-package allowlist. No commits were made.
