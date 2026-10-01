# B5-30 on-screen edge / alpha confirmation — 2026-10-01, Machine B

**Partial evidence: baseline red, alpha and 50% agreement PASS. The requested 150% and 75% checks are BLOCKED / unmeasured, not PASS or FAIL.** No renderer failure was established. No production or test source was changed.

## Build and method

- Branch: `wp/B5-30-evidence`; measured source: `dc30a179a277c2386cb3d59c5fca501eddaee839`.
- `PATH="$HOME/.cargo/bin:$PATH"; cd apps/mac && ./build-ffi.sh && Support/make-app.sh release` completed successfully. Packaging reported `Verified release dc30a179a277c2386cb3d59c5fca501eddaee839`.
- Exact app: this worktree's `apps/mac/build/Tessera.app`. Built-in Liquid Retina XDR, Apple M4 Max, 3456×2234 Retina.
- Generated all six fixtures with `tools/orchestrate/wp/B5-30/make-fixtures.swift`. The output directory must exist first; the first invocation failed at destination creation, then succeeded after `mkdir -p`. `sips -g profile` confirmed Display P3 for all `*-p3.png` fixtures and sRGB IEC61966-2.1 for all `*-srgb.png` fixtures.
- Every fixture used `open -g -n <exact-app> --args --nonactivating --app-dir <scratch>/p3b/app-<name> --open-document <fixture> --filter-selftest=<scratch>/p3b/dummy`. The equals-form self-test flag created the background host without starting a filter self-test.
- Window IDs and owning app paths came from `CGWindowListCopyWindowInfo` and `NSRunningApplication`. All observations of the frontmost app showed `/Applications/Claude.app`. Tessera was never raised, activated, resized, or made full-screen. No desktop keyboard/mouse input was used.
- Reused the supplied `wincap` and `px` helpers. Both abort without arguments, so their adjacent Swift sources were inspected for usage. `wincap <id> <png> p3` explicitly selects Display P3; every original capture is 2880×1968 and tagged Display P3. `px <png> <x> <y> ...` draws into the capture's own colour space and reads RGB with top-left coordinates.
- App-targeted `cua_repl` `pressKey('super+minus')` worked without activation: 100% → 66.7% → 50%, confirmed by the accessibility status text. Menu inspection worked, but selecting the 100% item failed twice with “element ID is no longer valid,” including after a refresh.

## Measured results

RGB below is **Display P3 capture RGB**, not an sRGB meter conversion. All readings, including supplemental diagnostic samples, are retained in [ONSCREEN-MEASUREMENTS.txt](ONSCREEN-MEASUREMENTS.txt).

| Fixture | Zoom | Measured RGB | Verdict |
|---|---:|---|---|
| red P3 | 100% | red `(255,0,0)`; grey `(128,128,128)` | **PASS** gamut / grey baseline |
| red sRGB | 100% | red `(234,51,35)`; grey `(128,128,128)` | **PASS** gamut / grey baseline |
| alpha P3 | 100% | dark checker `(120,117,113)`; light checker `(169,168,165)` | **PASS** linear-light alpha baseline |
| alpha sRGB | 100% | dark checker `(120,117,112)`; light checker `(169,168,165)` | **PASS** linear-light alpha baseline; P3 agreement within 1 |
| half-step P3 | 50% | four samples `(128,128,128)`; then `(128,129,129)`, `(127,128,128)` | **PASS**, agreement only, 128±1 |
| half-step sRGB | 50% | four samples `(128,128,128)`; then `(128,129,129)`, `(127,128,128)` | **PASS**, agreement only; exact P3 sample agreement |
| half-step P3 | 100% | lower split `(0,0,0)`, `(255,255,255)` | Control only; no interpolation verdict |
| half-step sRGB | 100% | lower split `(0,0,0)`, `(255,255,255)` | Control only; no interpolation verdict |
| half-step P3 | 66.7% | row y=970, x=1300…1305: `(178,178,178)`, `(217,217,217)`, `(175,175,175)`, `(145,145,144)`, `(196,195,196)`, `(215,214,215)` | Diagnostic only; capture resampling confound |
| half-step sRGB | 66.7% | same six RGB values as P3 | Diagnostic only; profile agreement, not the requested 150% edge proof |

At 100%, red/grey and half-step split points are `(1250,1000)` and `(1500,1000)`. Alpha uses those same points in reverse checker order: first light, then dark. At 50%, the six points are x=`1300,1301,1302,1400,1401,1402`, y=`990`.

Alpha dark-checker readings are within 2 of the requested approximate `(121,118,113)` and far from the encoded-blending failure value `(83,81,78)`. The light readings are within 1 of `(169,167,164)`. These are flat checker interiors. Exactly 50% uses the engine's level-1 byte-averaged pyramid, so its agreement does **not** prove linear interpolation.

## Requested zoom matrix still outstanding

| Fixture | Zoom | Measured RGB | Verdict |
|---|---:|---|---|
| red P3 | 150% | — | **BLOCKED / unmeasured** |
| red P3 | 75% | — | **BLOCKED / unmeasured** |
| red sRGB | 150% | — | **BLOCKED / unmeasured** |
| red sRGB | 75% | — | **BLOCKED / unmeasured** |
| half-step P3 | 150% | — | **BLOCKED / unmeasured** |
| half-step P3 | 75% | — | **BLOCKED / unmeasured** |
| half-step sRGB | 150% | — | **BLOCKED / unmeasured** |
| half-step sRGB | 75% | — | **BLOCKED / unmeasured** |
| alpha P3 | 150% | — | **BLOCKED / unmeasured** |
| alpha P3 | 75% | — | **BLOCKED / unmeasured** |
| alpha sRGB | 150% | — | **BLOCKED / unmeasured** |
| alpha sRGB | 75% | — | **BLOCKED / unmeasured** |

`DocumentViewportMath.steps` includes 50%, 66.7%, 100%, 200%; it includes neither 75% nor 150%. The View menu offers Zoom In, Zoom Out, Fit on Screen and 100%; the percentage display is static text. Source inspection found no arbitrary-zoom launch argument or self-test hook. A temporary launch-only zoom hook was requested from the coordinator/user, but no response was received during this run, so no app modification was made. These missing measurements must not be inferred from the passing offscreen tests or the baseline checks above.

The original ScreenCaptureKit helper also resamples the window: in a 100% red capture, the nominal 512-pixel fixture spans approximately x=1108…1586 (about 480 pixels), with blended outer edges. This is material for one-pixel interpolation checks. A scratch copy of the helper was tried with `ignoreShadowsSingleWindow = true`, `.best` resolution and dimensions derived from `contentRect × pointPixelScale`; it reported 1440×984 points at scale 2 but produced the same diagnostic samples. Disabling shadows did **not** resolve the resampling. A future exact-edge run must verify a 1:1 capture path as well as reach the exact zoom. The 66.7% diagnostic is therefore deliberately not assigned the 150% midpoint / adjacent-sum PASS criterion.

## Capture packaging and cleanup

For each measured fixture/zoom, `onscreen-<fixture>-<zoom>-overview.png` is a downscaled 432-pixel-wide contextual capture. `onscreen-<fixture>-<zoom>-pixels.png` is a **native, unscaled** crop of original capture rectangle `(1200,940,340,100)`. To locate a listed sample in a crop, subtract `(1200,940)` from its original coordinates. Every recorded sample in each retained crop was re-read with `px` and asserted exactly equal to the original capture. Use the pixel crops, not the resized overview images, for RGB review.

Original full captures remain in `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/p3b/`. Existing earlier evidence PNGs were preserved. The complete dated evidence directory, including earlier PNGs, is under 10,000,000 bytes.

All ten owned Tessera instances were terminated by their exact recorded PIDs: `12487,13349,13390,13422,13481,13512,13625,13975,14658,14972`. No evidence-worktree Tessera process remains. A separate agent subsequently launched `/Users/rutmehta/Developer/lightroom/.worktrees/B5-40/apps/mac/build/Tessera.app` (PID 16797); it was not launched by this verification run and was left untouched, as required by the ownership boundary. No push was performed.
