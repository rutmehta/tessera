# M5-32 partial implementation — blocked, not ready to merge

No commit made. All source edits remain inside the requested allowlist.

## Actual gate result

The parent ran the exact requested chained gate with the exported
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32`.
It exited 101 in the initial release test compilation:

```
error[E0027]: pattern does not mention fields `source_filename`, `dither`
 --> crates/compositor/src/resident/program.rs:493:13
493 | Adjustment::ColorLookup { size, data } => {
```

The clippy, fmt, workspace check, FFI build and Swift build stages were not
reached by that `&&` chain. No release-suite success is claimed.

## Ownership conflict requiring coordination

- `crates/compositor/src/resident/program.rs` must consume the added adjustment
  fields; ignoring them fixes compilation only, not dither/neutralize GPU parity.
  GPU behavior changes also require the owning resident/render worker.
- `crates/tessera-ffi/src/document/channels.rs:276` exhaustively matches
  ChannelKind and needs AlphaDisplay handling.
- `crates/tessera-ffi/src/document/tools.rs:1329` matches StrokeTarget and needs
  the channel destination.
- RasterFilterOperation and dispatch now live in
  `crates/tessera-ffi/src/document/filters.rs`, while the neural filter bridge
  lives in `document/retouch.rs`. These are excluded by the current brief;
  `document.rs` primarily re-exports their types.

No forbidden files were edited to bypass these boundaries. Request owner
integration or expanded permission before continuing.

## Partial source changes

- Adjustment serde defaults: ColorLookup source_filename/dither; MatchColor
  neutralize; Auto shadow_clip/highlight_clip and histogram constructor.
  CPU lookup dither and gray-world Lab neutralization implementation added.
  Exact Photoshop neutralization equivalence has not been established.
- AlphaDisplay color/opacity/polarity variant, native serialization through
  channel kind and PSD channel-resource mapping, validation.
- PaintTarget::Channel, DocOp/paint_op channel destination, brush helper,
  engine-api StrokeTarget::Channel and MCP wiring.
- Contract version constant changed from 1.5.0 to 1.6.0; contract changelog and
  compositor documentation still need updating/review.
- Dedicated adjustment/channel tests in compositor, brush, engine-api and MCP.
- An ignored 18MP Remove performance regression test was added. No Remove
  optimization was implemented and no timing measurement was obtained.

## Test evidence limitations

Subagents reported focused adjustment/channel tests passing using a temporary
source overlay that added `..` to the excluded renderer pattern. That overlay
was removed. These are NOT evidence that the actual worktree builds; the
parent's unmodified-source gate above failed. MCP execution has not been verified.

## Remaining work

1. Resolve excluded renderer and FFI integration ownership.
2. Complete ColorLookup PSD filename/Dthr mapping (currently old behavior).
3. Update existing adjustment constructors/patterns in compositor tests for
   new fields; verify defaults, native/PSD round trips and CPU/GPU behavior.
4. Implement and benchmark bounded PatchMatch Remove, preserving CAF quality.
5. Expose denoise-only PhotoRestoration on DocumentSession's existing raster
   dispatch, test missing-weights atomicity and regenerate bindings.
6. Complete channel/brush/MCP/FFI tests, documentation and contract changelog.
7. Run the entire requested gate without source overlays or test shims.
