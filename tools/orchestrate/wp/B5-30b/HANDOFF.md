# B5-30b — document colour follow-ups

Base: `origin/wp/B5-30c` at `2f024d9cff4e09486dbb4e37f34c44d314b0a719` (formerly `wp/B5-30@69728b71`). Rebased locally on 2026-10-01. Swift-only follow-up; engine pixels and Rust are unchanged.

## Behaviour and encoding boundaries

- Channels component overlays, RGB/component channel thumbnails and layer thumbnails carry `DocumentDisplayColor.space`, exactly like the detail preview panes. These CGImages contain original document-encoded bytes; the async layer thumbnail loader carries that profile into image creation without converting the bytes. Alpha/spot channel masks remain scalar grayscale, and layer masks retain their existing display path.
- B5-30c remains authoritative for rendering: sRGB-TRC profiles (including tagged Apple/HP sRGB and Display P3) use hardware sRGB decoding and the linearized-twin Metal layer; other TRCs use the original document profile and encoded sampling/compositing. Untagged/built-in sRGB retains its extended-linear EDR configuration. IOSurface metadata and CGImages describe the original encoded samples, not the decoded Metal values. Engine mip generation is unchanged.
- The P3 regression checks the linearized P3 layer, `.rgba8Unorm_srgb`, EDR off, and successful drawable allocation. Non-sRGB-TRC coverage retains the encoded-profile layer and `.rgba8Unorm` assertions from 30c.
- B5-30c's ICC-digest refresh and TRC classification are retained. Same-name/different-ICC profile changes continue to retag the retained viewport ring and layer.
- An unsupported embedded ICC still logs the sRGB fallback. The controller retains the diagnostic until a status callback exists, then delivers it on the next main-actor turn, after the workspace's synchronous “Opened …” status. Delivery consumes the warning once. An unchanged digest must not erase an undelivered warning; the regression explicitly reloads the model before installing the callback.

## Rebase resolution

- `DocumentDisplayColorTests.swift`: retained 30c's linearized P3/EDR-off/drawable/texture assertions while preserving 30b's channel, thumbnail and deferred-diagnostic tests.
- `DocumentBackend.swift`: retained the explicit document-profile encoding comments, including sRGB when untagged.
- `DocumentController.swift` merged textually but required a semantic fix: reset the pending diagnostic only after the digest changes. Initialization and subsequent unchanged-profile refreshes otherwise discard the queued opening warning.
- Rewrote this handoff to replace the old B5-30 encoding assumptions and superseded capture blocker with the 30c model and current qualification results.

## Validation

Current serial qualification on 2026-10-01 **passed on the first run**:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

- FFI archive/bindings built successfully for arm64; Swift debug build succeeded.
- Release test build succeeded (318.83 seconds).
- XCTest: 875 tests executed, 3 skipped, 0 failures (309.746 seconds).
- Swift Testing: 5 tests in 2 suites passed.
- All 11 `DocumentDisplayColorTests` passed, including unchanged-digest deferred-warning delivery, P3 SDR drawable allocation, overlay/component/async layer thumbnail tagging, non-sRGB TRCs, same-name profile retagging, and rendered half-step/alpha acceptance.
- Offscreen `MasksPanelLayoutTests` passed. The full gate printed **`SWIFT GATE OK`** and exited 0. No rerun was needed; no tests were excluded or weakened.
- `git diff --check` passed.

Historical evidence before rebase: original RED `5c72c345` produced 10 assertions in 6 targeted tests; fix `87d8d02d` passed all 6. The old full-gate attempts failed in History/layout tests and encountered an on-screen capture helper. Those results do not qualify this rebased branch. B5-30c supplies the offscreen layout OCR harness used for the current gate.

No GUI app launch. No Rust, `Cargo.lock`, or `board.json` edits. All commits remain local; no push.
