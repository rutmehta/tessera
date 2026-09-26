# M2-09b: staged embedded optics and manual CA

## Integration / GPU fallback

`ResolvedLens::plan` returns `Ok(None)` for **all supported embedded opcodes**, even a single List3 warp/gain, and for nonzero independent manual CA. Resident callers must use the CPU reference fallback. Existing defringe fallback remains. Do not flatten these lists into the legacy resident `EmbeddedWarp`/`EmbeddedGain` arrays.

`LensContext` gained `manual_ca: ManualCaSettings` (Default is identity). Existing callers using `..Default::default()` remain source-compatible. `resolve_lens` and `resolve_lens_sensor` carry this setting into `ResolvedLens`; `render_linear_scaled_resolved` therefore retains it. `ResolvedLens::from_calibration` defaults it to zero. No engine-api changes or lens-root exports are needed.

## Stage contract

- Raw-decode already preserves selected-IFD OpcodeList1/2/3 bytes and does not apply corrections. No decoder change was needed.
- List1 executes on normalized full-sensor CFA **before highlight reconstruction/demosaic**. Warp sampling uses the destination phase lattice (Bayer period 2, X-Trans period 6), so green phases and other colours never mix.
- List2 executes on full-sensor linear camera RGB after demosaic and automatic profile CA, before denoise/camera profile/white balance.
- Independent manual CA and hue-selective defringe execute in the lateral pass after List2, before channel-mixing matrices. For RGB sources they precede white balance. Defringe is not repeated by the later manual vignette pass.
- List3 executes after camera profile and white balance, before active-area extraction/detail/tone. Its channels are post-colour channels, not camera channels.
- Within each list, every warp/gain is executed in file order against the previous result. Warp-then-vignette is supported without commuting or reversing the operations.
- Warps and radial vignettes retain existing distortion/CA/vignette amount controls. GainMap is raw calibration and applies its complete gain independently of vignette amount.
- Embedded wins over database/image estimation in Auto/Embedded mode. Explicit None/Database/AutoCalibrated selections retain their prior override semantics. Embedded bytes are parsed even when not selected; malformed/unknown required operations fail closed. Required bad-pixel IDs 4/5 remain deliberately ignored as before.
- Staged operations are not inserted into the old deferred geometry/profile-gain collections, so they cannot apply twice. Caller-injected database resolution does not override active embedded metadata in the CFA renderer.

## Supported opcode payloads

1 WarpRectilinear: one shared or three distinct channel coefficient sets; off-centre, aspect-aware full-sensor coordinates; bilinear resampling. Two-plane warps remain unsupported.

3 FixVignetteRadial: five polynomial coefficients, normalized optical centre, executed at its declared stage.

9 GainMap: bounded big-endian AreaSpec, plane range, row/column pitch, 2D map dimensions, normalized spacing/origin, interleaved map planes, positive finite float gains, bilinear interpolation and clamped map edges. CFA maps address physical plane 0 with pitch/area selecting phases. Invalid ranges, nonfinite values, truncated lengths and dimension overflow are rejected. Other required unknown IDs fail closed; optional unknown IDs are skipped.

## Recipe / sidecar work for parent

Only two engine-api fields are missing. Proposed persistent fields:

| Future recipe field | CPU-owned field now | CRS name |
|---|---|---|
| `/settings/lens/manual_ca_red_cyan` | `LensContext.manual_ca.red_cyan` | `crs:ChromaticAberrationR` (currently Legacy) |
| `/settings/lens/manual_ca_blue_yellow` | `LensContext.manual_ca.blue_yellow` | `crs:ChromaticAberrationB` (currently Legacy) |

Both are finite floats, zero identity, clamp -100..100. A slider unit is 0.0001 radial scale; +100 samples 1% farther from the active-area centre. This is Tessera's explicit reference convention, not a claim of Adobe numerical parity. Neither setting is gated by `remove_chromatic_aberration` or `chromatic_aberration_scale`; they are additive to embedded/profile correction. Parent must persist/hash these settings when adding schema integration.
`ManualCaSettings::from_crs` maps numeric `crs:ChromaticAberrationR` and
`crs:ChromaticAberrationB` properties now; unrelated keys are ignored.
The resolved-render path preserves these manual values when embedded calibration
replaces an injected profile.

Defringe fields **already exist** and need no duplicate context fields: `defringe_purple.amount/hue_range`, `defringe_green.amount/hue_range`. CRS names: `DefringePurpleAmount`, `DefringePurpleHueLo`, `DefringePurpleHueHi`, `DefringeGreenAmount`, `DefringeGreenHueLo`, `DefringeGreenHueHi`. Amount 0..20; CPU hues are degrees with wrapping intervals; sidecar already multiplies CRS 0..100 hue endpoints by 3.6. Fixed full-circle `[0,360]` handling (formerly normalized to an empty/single-hue interval).

## Verification

Test-first failures observed for GainMap parsing, early-list acceptance, staged gain execution, independent manual CA, and full-circle defringe. Handbuilt fixtures also cover phase isolation, interpolation/origin/pitch/area, opcode order, stage-dependent matrix mixing, no duplicate gain, malformed bytes and resident fallback.

Passed: `cargo test -q -p pipeline-cpu` (including raw fixture goldens; one existing full-resolution acceptance test remains ignored), `cargo test -q -p raw-decode --lib` (15), `cargo test -q -p lens --lib opcodes` (4), and `git diff --check`.

The real-fixture opcode gate inspected all five supplied RAWs and found no opcode
payloads, so none could exercise staged execution. Hand-built lists supply that
coverage. Future opcode-bearing fixtures are rendered by `opcode_fixtures.rs`.

All cargo commands used `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-09b`. No commits. Final combined verification is recorded in tools/orchestrate/wp/M2-09b/HANDOFF.md.
