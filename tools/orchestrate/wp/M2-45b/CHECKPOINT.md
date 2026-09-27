# M2-45b checkpoint: output sharpening

Overall WP result: FAIL until the remaining items below are delivered. This
checkpoint implements item 2 only, using the brief's permission to stop after
a green item. It does not claim metadata/DNG/HDR/workflow completeness.

## Implemented

- Engine `SharpenAmount::{Low,Standard,High}` for all Screen/Matte/Glossy media.
- Deterministic separable Gaussian unsharp mask after final resize and before
  watermarking, shared by the CPU/GPU-rendered export paths. Standard retains
  the old 300-ppi coefficients. Low/high multiply amount by 0.5/1.5.
- Paper radius scales with output ppi / 300, bounded to 0.3–8 pixels. Screen
  radius is fixed in pixels. Valid explicit density: 1–9600. Missing engine/CLI
  density uses 300 ppi without writing a new density tag. FFI uses its existing
  dpi setting (default 72). These are Tessera presets, not Adobe pixel parity.
- CLI `--sharpen-amount low|standard|high` and `--ppi`, with existing `--sharpen`.
- Backward-compatible FFI/preset JSON `sharpening_amount`, default `standard`.
  Existing export settings continue to parse. No new Swift UI or UniFFI ABI.
- Tests: all nine combinations on an edge chart, monotonically increasing
  enhancement, constant preservation, repeatability, density response, invalid
  density and cancellation. TIFF16 read-back checks resize-then-sharpen order
  at 150/300 ppi. CLI parsing, FFI defaults/serialization, preset storage and
  actual exported pixel differences are covered.

The legacy print `render_pixels` API retains standard/300-ppi behavior. The
public `sharpen_output` function permits explicit controls on unsharpened
renders. MCP's minimal export schema still does not expose sharpening controls
and continues to use no sharpening. This checkpoint does not add UI controls.

## Verification

Red/green evidence (local ignored logs):
- `sharpen-red.log`: missing amount enum/parameterized filter before implementation.
- `sharpen-green.log`: chart test passes.
- `wiring-red.log`: missing public API/export setting before integration.
- `wiring-green.log`: TIFF16 integration passes.
- `hosts-red.log`: CLI rejects the new flag before wiring.
- `ffi-red.log`: normalized JSON lacks the new default before wiring.

Full gate result is recorded in `verification.txt` after execution; `gate.log`
is the full local log. `CARGO_TARGET_DIR` remains
`/Volumes/betterSSD/tessera-cache/target/M2-45b` throughout.

## Still required (not attempted in this checkpoint)

1. Metadata: copyright+contact, all except camera/Camera Raw, person/location
   removal, Lightroom hierarchical keyword policy, and EXIF/IPTC/XMP coverage
   with policy read-back tests in JPEG/TIFF/PNG/AVIF/JXL/DNG. Existing XMP-only
   all/copyright/none policies are unchanged.
3. DNG: original+XMP copy, original raw embedding toggle, DNG 1.6 tag audit, and
   independent LibRaw RGB comparison. Existing developed float DNG remains at
   the M2-45 implementation and validation level.
4. HDR: PNG16 and AVIF10/12 PQ/HLG with Rec.2020 CICP/ICC, interoperable ISO
   21496-1 JPEG gain maps and reconstruction/tag tests.
5. Workflow: structured host-executed reveal/open/script actions, persisted
   Export with Previous, and multi-preset export execution.

Lossy JPEG XL remains out of scope; no GPL codec or new dependency was added.

## Integration hotspot

`crates/export/src/lib.rs` is also named in the concurrent M2-49 worker's scope.
This change only adds the amount setting/default/re-export and updates the
existing final sharpening call. Preserve both workers' independent changes
when merging. No other worktree was modified.
