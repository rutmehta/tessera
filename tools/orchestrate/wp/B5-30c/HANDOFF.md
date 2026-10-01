# B5-30c — linear-light sampling for sRGB-transfer document profiles

Branch `wp/B5-30c`, based on `wp/B5-30` at `69728b71`. Local commits only; no Tessera app launch, foreground interaction, or desktop screenshots.

## Delta from B5-30

B5-30's claim that tagging alone makes opaque pixels exact was too broad: interpolation of encoded
samples darkens high-contrast edges even at alpha 1. This package retains hardware sRGB decoding for
profiles whose transfer curves match sRGB, while preserving the document's primaries in the layer tag.
The engine still supplies document-encoded RGBA8 straight-alpha samples. No Rust, shader colour math,
export pipeline, board.json or Cargo.lock changes are required.

| Profile class | Texture sampling | CAMetalLayer colour space | EDR |
|---|---|---|---|
| Untagged / engine built-in sRGB | `rgba8Unorm_srgb` | extended linear sRGB, unchanged | true |
| sRGB-like TRC: Apple/HP sRGB, Display P3, equivalent custom ICCs | `rgba8Unorm_srgb` | `CGColorSpaceCreateLinearized(documentSpace)` | false |
| Non-sRGB TRC: Adobe RGB gamma 2.2, ROMM/ProPhoto 1.8, linear, etc. | `rgba8Unorm` | original document profile | false |
| Valid RGB profile without a CoreGraphics linearized twin | `rgba8Unorm` | original document profile, plus diagnostic | false |
| Invalid / non-RGB profile | sRGB fallback | extended linear sRGB, plus diagnostic | true |

Detection samples the document's grey ramp into its linearized twin and compares all three RGB
components against the sRGB decode curve. This handles both tabulated and parametric ICC TRCs and
ignores profile names. The tolerance is 0.001 absolute linear-light error at each of 256 samples
in each RGB component; gamma 2.2 differs by more than 0.008. Only a matching curve permits hardware sRGB decode.

**Remaining limitation:** non-sRGB-transfer and unavailable-twin profiles still interpolate and blend
in their encoded component space. Tagging makes a single sample colour-correct but cannot correct
encoded interpolation or transparency for nonlinear curves. An already-linear profile needs no decode:
its native components are linear-light even on `rgba8Unorm`. No TRC LUT shader has been added. Preview images still use
CoreGraphics/AppKit presentation; this package does not assert identical preview resampling or alpha
compositing to Metal.

Ring IOSurface metadata and all four preview CGImages (Filter, Camera Raw, Liquify, Adaptive Wide Angle)
retain the original **encoded** document profile because their bytes are still encoded. Only the Metal
sampling view decodes, and only its output layer gets the linearized tag. Ring texture views are rebuilt
when the sampling format changes; retained surfaces are re-tagged whenever the profile bytes change.

The controller checks a SHA-256 digest of ICC bytes on model/history refresh, replacing name-based
invalidation. Theme colours convert to the actual layer space. Conversion failure emits a diagnostic
and uses document-space black; it cannot fall through to unrelated linear-sRGB component values.

## Verification

RED commit: `1bec4aba` (`test(B5-30c):`). The only production edits in that commit expose the existing
render pass and theme conversion for testing, plus a linearizer injection parameter; sampling policy
and conversion behaviour remain B5-30. Run:

```sh
cd apps/mac
swift test -c release -Xswiftc -enable-testing --filter DocumentDisplayColorTests
```

Result: **8 tests, 14 assertion failures, exit 1**. The `(0 unexpected)` text did not mean success.
Relevant lines in `DocumentDisplayColorTests.swift` at RED:

- `:243`: Apple/HP sRGB and Display P3 midpoint `127.5` != `187.5 +/- 2`; alpha
  `83.173828125` != `122 +/- 2` (four failures).
- `:120`, `:125`: P3 layer carried encoded ICC (584 bytes vs linear twin's 400), and texture format
  was 70 (`rgba8Unorm`) instead of 71 (`rgba8Unorm_srgb`).
- `:162`, `:163`, `:167`: same-name ICC change left controller, layer, and all three ring tags on P3
  (536 bytes), instead of Adobe RGB (560 bytes).
- `:199`, `:200`: failed theme conversion returned red linear-sRGB components instead of safe black,
  and emitted zero diagnostics instead of one.
- `:176`: missing linear twin produced no diagnostic.

The builtin sRGB control passed, measured at half-step **187.5160/255** and alpha **121.3827/255**.
Each pixel test renders a real IOSurface-backed texture with the production Metal encoder, sampler,
and fragment shader into a 1x1 RGBA16F offscreen texture at 50% zoom. After GPU completion, it reads
that texture and converts from the layer's colour space to the document's encoded space for comparison.
Alpha input is 128/255 (the representable half-alpha in RGBA8) over an sRGB theme grey of 167/255.

GREEN: the same 8 tests passed, zero failures, exit 0. Measured encoded output (0–255):

| Document | Half-step, opaque black/white | Black at alpha 128/255 over grey 167/255 |
|---|---:|---:|
| Engine built-in sRGB | 187.516032 | 121.382708 |
| Apple/HP sRGB ICC (`/System/Library/ColorSync/Profiles/sRGB Profile.icc`) | 187.516032 | 121.382708 |
| Engine Display P3 ICC | 187.516032 | 121.382708 |

All are within the required 2/255 tolerance of 187.5 and approximately 122. These are offscreen GPU
measurements, not calculated substitutes or screen captures. The P3 integration test also obtained an
actual `CAMetalLayer.nextDrawable()` with the linearized P3 tag and asserted EDR false. Same-name ICC
replacement re-tagged all three retained ring surfaces without replacing their IOSurface IDs. Adobe
RGB, ROMM/ProPhoto, and linear RGB stayed on the encoded path. The nil-twin and theme-conversion
failure paths passed their diagnostic/fallback checks.

Logs for this run: `/tmp/B5-30c-red.log`, `/tmp/B5-30c-green.log`, `/tmp/B5-30c-gate.log`.
Fix commit: `adb1d124` (`fix(B5-30c):`).

The first full required gate failed: 869 XCTest cases, 3 skipped, one failure in
`ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`; the additional five Swift
Testing cases passed. The eight colour regressions passed inside this full run. An unchanged isolated
layout rerun also failed at `ShellLayoutTests.swift:194`, reporting stale History body/header geometry
at 1440x900 and 1728x1117. Multiple other XCTest processes were active; these layout tests share
`UserDefaults.standard` History keys. While this worktree had no running test process (the second gate was still compiling),
`defaults read com.apple.dt.xctest.tool InspectorPanel.History` changed from 1 to 0. That confirms
external writes to the exact shared preference used by this test. Interference is the likely explanation
for the layout failure; its precise failing instant was not instrumented. No layout code or test
expectation was changed. Details: `/tmp/B5-30c-layout-repro.log`.

The second full gate passed the inspector-layout test unchanged, but failed
`MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth` at line 88:
macOS reported `could not create image from window`, so the existing background-window capture/OCR
path had no image. It again ran 869 XCTest cases (3 skipped, 1 failure) and five passing Swift Testing
cases. Full retained log: `/tmp/B5-30c-gate2-tests.log`.

Separate test-infrastructure commit: `45106a4f` (`test(B5-30c):`). It changes no production layout and
removes no assertions or tests:

- `ShellLayoutHarness.prepare()` installs a private per-process preferences store through the existing
  `AppDefaultsIsolation` API. Concurrent worktree gates cannot toggle this inspector's History state.
  The previously failing inspector test passed (1 test, 0 failures) after this change.
- The Masks test now feeds Vision OCR from `NSHostingView.cacheDisplay` into an offscreen bitmap, instead
  of calling `screencapture`. The same full-word assertions for Components/Add/Subtract/Intersect pass
  at both 288/380 widths and light/dark appearances (1 test, 0 failures). Optional diagnostic PNGs also
  come from that bitmap. Other harness captures remain opt-in; `TESSERA_LAYOUT_CAPTURE` is unset for
  the final gate.

Evidence: `/tmp/B5-30c-layout-isolated.log`, `/tmp/B5-30c-masks-offscreen.log`.
A subsequent full run stalled after the offscreen Masks check had passed. A one-second sample of
that owned XCTest process showed **80 `NSAnimation _runBlocking` workers**, dispatch's thread soft
limit reached, and XCTest waiting for an async test. It was terminated rather than called a pass.
Evidence: `/tmp/B5-30c-gate3-sample.txt`, `/tmp/B5-30c-gate3-tests.log`.

An additional harness regression (`ff157136`, `test(B5-30c):`) first failed at
`ShellLayoutTests.swift:35`: settling a real `NSProgressIndicator` left threaded animation enabled.
The test-only fix (`33d1c61e`, `fix(B5-30c):`) walks layout views and selects the documented timer-based
animation mode (`usesThreadedAnimation = false`). This selects timer mode for background progress indicators, but the subsequent full run
still reached 80 blocking animation workers; this change alone did not resolve the suite hang. It applies to the shell harness and offscreen Masks fixture, not
production views. The new policy regression and unchanged Masks OCR test passed together (2 tests,
0 failures). Logs: `/tmp/B5-30c-animation-red.log`, `/tmp/B5-30c-animation-green.log`.

The fourth run (`/tmp/B5-30c-gate4-tests.log`) passed ShellLayout but stalled at
`SmartPreviewThumbnailTests.testCancelledRetryWaitTerminatesWithoutAnotherNativeCall`; its sample
again showed 80 blocking animation workers. The owned test process was terminated, not passed.
It also exposed an existing Adaptive Wide Angle race: a slider trace can finish after failed OK,
schedule a successful preview and erase the final apply error. Regression commit `8fb874d2` failed
at `DocumentAdaptiveWideAngleTests.swift:403` with nil instead of the final constraint error
(`/tmp/B5-30c-awa-red.log`). OK now invalidates both pre-existing preview and trace generations
before starting the final apply job.

The wide-angle fix (`50912455`) passed its regression and the full AWA suite together with
Agent Review layout and thumbnail tests: 31 tests, zero failures
(`/tmp/B5-30c-awa-green-animation-probe.log`).

The smaller combination AgentReviewLayout + MasksPanelLayout + ShellLayout + SmartPreviewThumbnail
reproduced the hang (`/tmp/B5-30c-layout-animation-probe.log`). Process samples showed blocking
animation workers accumulating from 16 to 59 to 80 across those suites. Explicit native progress
stops and window disposal (`c2817061` RED, `40151b12` fix) passed their policy assertion but did not
by themselves resolve the hang (`/tmp/B5-30c-layout-animation-fixed.log`). SwiftUI also creates
NSAnimation objects outside NSProgressIndicator view descendants. `c96fb481` adds a direct RED
regression: a threaded NSAnimation remained threaded at `ShellLayoutTests.swift:31`
(`/tmp/B5-30c-nsanimation-red.log`). Fix `b591b001` installs a test-process-only start hook that
selects AppKit's public nonblocking mode before calling the original animation implementation.
This applies only after `ShellHarness.prepare()` in XCTest; production binaries are unchanged.
The previously hanging four-suite combination now passes: 19 tests, zero failures in 132 seconds,
including all 10 thumbnail tests after all layout cases (`/tmp/B5-30c-nsanimation-green.log`).
A sample during review layout found no `NSAnimation _runBlocking` workers
(`/tmp/B5-30c-nonblocking.sample`).

Final required gate, 2026-10-01: **SWIFT GATE OK**, exit 0.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export NSUnbufferedIO=YES
unset TESSERA_LAYOUT_CAPTURE
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

Exact gate summary:

```text
Executed 872 tests, with 3 tests skipped and 0 failures (0 unexpected) in 239.000 (239.072) seconds
Test run with 5 tests in 2 suites passed after 0.024 seconds.
SWIFT GATE OK
```

The three existing opt-in skips were unchanged; no tests were disabled to pass the gate.
All eight colour tests ran and reproduced the measured table above in this full run. The wide-angle,
Masks OCR, shell layouts and subsequent thumbnail tests all passed. Final logs:
`/tmp/B5-30c-gate5.log`, `/tmp/B5-30c-gate5-tests.log`.
Rust, Cargo.lock, board.json and the gate script are unchanged. Builds were serial in this worktree;
other worktrees' processes were neither controlled nor terminated. No push or merge was performed.

## Fixtures for the coordinator

Generated files are included in `tools/orchestrate/wp/B5-30c/fixtures/`. Reproduce with
`swift tools/orchestrate/wp/B5-30/make-fixtures.swift /tmp` to produce all three fixture pairs:

- `b5-30-red-{p3,srgb}.png`: original saturated-red / grey comparison.
- `b5-30-half-step-{p3,srgb}.png`: upper half alternates black/white every source pixel; lower half
  splits at source x=255.
- `b5-30-alpha-{p3,srgb}.png`: black at alpha 128 on transparent.

The generator was run in this worktree. `sips` confirmed 512x256 and the P3/sRGB profiles for all
six files; decompressed PNG scanlines are byte-identical within each pair. ImageIO writes P3 as an
`iCCP` chunk and sRGB as a standard `sRGB` chunk on this macOS version. Thus the sRGB PNG is a reference
for standard sRGB; the actual Apple/HP ICC representation is separately exercised by the render test.
This corrects the original B5-30 handoff's assumption that the generated sRGB PNG necessarily embeds
Apple's ICC bytes.

At exactly 50% zoom, `level(forZoom: 0.5) = 1`: the viewport selects the engine's
half-size level, built by averaging raw 8-bit values (`mip_exact` / `mip.wgsl`).
The half-step edge therefore reaches Metal as 128 for both profiles regardless of B5-30c.
The 187.5 offscreen result above is valid proof of the GPU path, not what 50% shows.
B5-30c's visible effect is at zooms BETWEEN levels below 200% (for example, 75%, 150%)
and for transparency over the checkerboard.

On a wide-gamut display, open each pair in Tessera. Use Digital Color Meter
**"Display in sRGB", 8-bit**:

1. Red pair at any zoom: P3 red more saturated, greys 128 both.
2. Half-step at 50%: both read 128±1 (agreement only).
3. Half-step at 150%: midpoint ≈187 and adjacent pairs sum ≈330–375 = PASS;
   ≈128 / sums ≈255 = FAIL; P3 and sRGB within ±2.
4. Alpha fixture: black at alpha 128 on transparent — expected over the dark checker
   (A7A39C) ≈121/118/113 pass vs ≈83/81/78 fail; light checker in dark theme
   ≈169/167/164 vs ≈115; light theme white ≈187 vs ≈127.

These are future on-screen checks, not results claimed by Machine B. Offscreen renderer
measurements above isolate the viewport sampler from backend pyramid selection and
WindowServer/display conversion.
