# M2-45b checkpoint: output sharpening

Subsequent workflow checkpoint: see `WORKFLOW.md` for persisted Previous,
multi-preset UniFFI export and host post-actions. The notes below describe the
earlier sharpening-only checkpoint; its item 5 status is historical. Overall
M2-45b remains incomplete until metadata, DNG enhancements and HDR are done.

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
renders. This checkpoint does not add UI controls.

### Retry: MCP sharpening controls

- MCP `export.settings` now accepts `sharpening` (`none`, `screen`, `matte`,
  `glossy`), `sharpening_amount` (`low`, `standard`, `high`) and `ppi` (1–9600).
  Absent/null fields preserve legacy no-sharpening, standard strength and
  300-ppi paper radius without adding a density tag. Screen is ppi-independent.
- All settings reach the existing shared resize-then-sharpen engine path.
  Unknown medium/strength and invalid density fail before source decoding,
  output publication or history writes. No new codec/dependency was added.
- The engine API uses optional strings for the two mode fields, validated at
  the MCP boundary. Its existing build-time schema mirror picks up these
  fields without changes to the out-of-scope MCP build script; supported
  values are documented in the generated schema descriptions.
- New MCP integration tests export an edge chart across all nine presets,
  read decoded PNG pixels, check default compatibility and density behavior,
  and verify schema exposure and invalid-input side-effect isolation.
- `mcp-sharpen-red.log`: low/standard produced identical pixels before wiring.
  `mcp-validation-red.log`: invalid density reached PNG decode before the
  new preflight guard. `mcp-sharpen-green.log`: all three tests pass.
- The previously failing slider/export test passed in isolation with 120/120
  L2 frames and 3.3 ms render p90. No assertion was weakened, fixture skipped,
  environment skip flag set, or scheduling code changed. This is not a claim
  that the earlier latency failure's cause has been identified or fixed.

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

The retry's first full gate (`retry-gate.log`) found an independent clock-boundary
failure in `disabled_ai_never_calls_backend_and_matches_default_export`.
Programmatic comparison of its failure arrays found equal lengths (1243 bytes)
and exactly one difference: JPEG byte 73, ICC header byte 35, seconds 32 versus
33. The codec builds a fresh LittleCMS profile for each export. The regression
now explicitly separates the exports by 1.1 seconds, checks the APP2 ICC segment
sequence/count and ICC signature, and compares every byte except the 12-byte ICC
creation date. No pixel, color transform, or other metadata bytes are excluded.
The corrected test passes (`icc-time-green.log`); no codec behavior changed.

The final retry gate passed in full, including the slider/export test, Clippy,
formatting, license check, regenerated FFI and Swift build: 331 tests passed,
0 failed, 15 ignored. See `retry-verification.txt` for this invocation's results
and `retry-final-gate.log` for the full local output. The original
`verification.txt` remains the previous attempt's historical record.

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
