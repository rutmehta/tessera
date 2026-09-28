# Bounded PSD UI validation — 2026-09-27

## Tested build and isolation

- Product source under test: `dc4073de34b2da17f556e2f237d3f8b6322b4f14` (Review preview refresh update). The current evidence-only worktree tip is `8ac2e1ae27951c842b9b9994cf9b16b4542f3997`; the parent verified the 24 source hashes against the tested product source.
- Gate result reported and independently verified by the parent: Release `swift test`, 509 XCTest executed, 1 skipped, 0 failed, plus 5 Swift Testing tests; direct process exit 0.
- FFI archive used: `apps/mac/build/ffi/libtessera_ffi.a`, SHA-256 `0a9b2de3dee742751da067147805715925ac005e7036d0268d36296dc25168ae`.
- Exact SwiftPM executable used: `/Volumes/betterSSD/tessera-cache/swiftpm/psd-current-ffi-release/arm64-apple-macosx/release/Tessera`, SHA-256 `6726d93c1a272f6ec019f03b2f78d5a677e55822862bc532597d09aefc4a5f28`.
- Two copied, uniquely identified validation bundles were used, both with disposable `/tmp` app-support paths and Sparkle update checks disabled. No user bundle or user catalog was edited:
  - `/Volumes/betterSSD/tessera-validation/psd-ui-8ac2e1ae/Tessera-PSD-UI-Validation-8ac2e1ae.app` (normal launch; no arguments), packaged executable hash after adding the bundle Frameworks rpath and ad-hoc signing: `f40ffd3f1dd4a8558a3b3cc6e4acf4b7ae5bc21c29e3c6c43962da21a63ae965`.
  - `/Volumes/betterSSD/tessera-validation/psd-ui-8ac2e1ae/Tessera-PSD-Diagnostic-Validation-8ac2e1ae.app` (launched with `--enable-stub-library`), packaged executable hash after ad-hoc signing: `592e3bf46d0318796d9ae8df830866837f4358789563aa911c042d2cff55631a`.
- `codesign --verify --deep --strict` succeeded for both bundles. Their configured support roots were `/tmp/tessera-psd-ui-validation/normal-support` and `/tmp/tessera-psd-ui-validation/diagnostic-support` respectively. The validation processes (PIDs 16567 and 22462) were closed after testing. User preview PID 57591 was not interacted with or closed.

## GUI results

The normal no-argument launch began in an empty Library (`No Folder`, `No images`, count 0). The ordinary view exposed no “Load 20,000 Stub Items” action. The Release app’s accessibility menu bar did not include a top-level Debug menu, so I cannot claim a separate Debug-menu check in this configuration. I also did not verify a separately labeled Debug menu in the diagnostic launch; I verified the gated Library button only. Source inspection shows both the Library button and Debug command item are inside `StubLibraryDiagnostics.isEnabled` checks.

In the fixture Library, I selected the generated JPEG and invoked Library ▸ Open in Layers… (⌘E). The confirmation sheet stated that the first open creates a rendered copy; confirming opened the document with one pixel layer. File ▸ Save Rasterized PSD Copy… presented the native save panel; I saved `psd-ui-rasterized.psd`. The document status reported “Saved a rasterized copy as psd-ui-rasterized.psd”. File ▸ Open Document… then opened that saved file as a second document. The reopened document displayed 320 × 240 px, 16-bit, embedded PSD profile, one layer. The UI screenshot showed the checker image and one pixel layer; it is present inline in the CUA interaction record, not exported to a local screenshot path.

The diagnostic bundle was launched with the explicit `--enable-stub-library` argument. Its empty Library visibly exposed the “Load 20,000 Stub Items” button while the accessibility state still showed `No Folder`, `No images`, and zero counts. I did not click it or load synthetic items. The CUA screenshot showing this empty diagnostic state is also inline in the interaction record rather than persisted as a local PNG.

## Fixture and output integrity

- Generated fixture: `/tmp/tessera-psd-ui-validation/fixture/photos/psd-source-checker.jpg`, 320 × 240 JPEG, 8,137 bytes.
- Fixture SHA-256 before launch: `24ec0418d1e0ffa42c095bde2425fe3cf280fc199ab5c02eca0229529d127345`.
- The same fixture SHA-256 was verified after PSD creation and reopen. The source JPEG remained unchanged.
- Saved PSD: `/tmp/tessera-psd-ui-validation/fixture/photos/psd-ui-rasterized.psd`, 1,229,596 bytes; `file` identifies it as an Adobe Photoshop Image, 320 × 240, RGBA, four 16-bit channels; SHA-256 `f6d59f1de4622bcf638afbb422a10e4fb5a306d19203be5f97869bba433c4cce`.

## Scope limits

This verifies the small-image Open in Layers → Save Rasterized PSD Copy → reopen path and confirms the source JPEG hash is unchanged. The document had one pixel layer and I made no additional layer operation; this is not a stress, large-file, cancellation, or complex smart-filter/transform rasterization test. No in-flight cancellation was inferred from a confirmation dialog. Diagnostic visibility was checked only; the 20,000-item action was never invoked. CUA provided visible screenshot captures in the interaction stream, but its API did not provide a filesystem export path for those captures.
