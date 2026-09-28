# Actual Swift/native offline workflow — source-only candidate

Patch: /tmp/tessera-smart-preview-swift-native-workflow.patch
Test source: /tmp/SmartPreviewNativeWorkflowTests.swift
No shared source changes or compilation. git apply --check passed against current feature checkout. Apply only after root reserves the compiler lane and accepts this source review.

Primary test uses actual EngineLibrary.open, SmartPreviewAPI.live and DevelopController adapters with a disposable COPY of the existing Sony ARW and isolated support directory. It captures native canonical folder identity online, builds Compact (1640x1092), disconnects the copied folder, opens the actual cached factory using a new Engine, verifies declared membership and selection write refusals, renders/edit exposure and WB via IOSurfaces without any window/app activation, closes/saves, reopens offline and verifies edits/render, restores copy, synchronizes and verifies Original route sees edits. Hashes protect source fixture and hidden copy/sidecars; no photo bytes enter Git. This is bridge/render evidence, not app GUI or interaction-speed acceptance.

Separate positive thumbnail test prewarms the ordinary 384px preview through EngineImageReference/Engine.embeddedPreview and checks ThumbnailLoader.render using a fresh offline cached library. Source inspection predicts failure: crates/tessera-ffi/src/preview.rs request_raw calls PreviewKey::for_source before any cache lookup; crates/previews/src/revision.rs:16 stats original. Existing cached JPEG therefore cannot be retrieved offline. The test deliberately expects a non-nil thumbnail, not the defective nil behavior; keep its result separate from primary workflow.

After native guard fix, matching archive/bindings generation and lane grant, from feature checkout:

```sh
TESSERA_SMART_PREVIEW_RAW=/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated swift test --package-path apps/mac --scratch-path /Volumes/betterSSD/tessera-validation/smart-previews/swift-full-final/build -c release --filter SmartPreviewNativeWorkflowTests.testActualSwiftBridgeOfflineLibraryRenderSaveReopenAndReconnect
TESSERA_SMART_PREVIEW_RAW=/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated swift test --package-path apps/mac --scratch-path /Volumes/betterSSD/tessera-validation/smart-previews/swift-full-final/build -c release --filter SmartPreviewNativeWorkflowTests.testActualCachedThumbnailAfterOriginalDisconnect
```

Capture exact source/archive/generated hashes, full command, direct exit and logs with existing evidence runner. Without fixture env, both tests skip. No success claim until actually compiled/run; potential mechanical Swift integration errors remain unqualified.

Patch SHA256: ffc31898b7c58416db5fcec3ee4c53f451f5f19205239e9342c0713bf59ac271

Source review correction: online baseline uses actual Original DevelopController.apply/commit/close, preserving recipe history. Offline thumbnail positive waits up to60seconds for asynchronous completion. Source-review approved; still uncompiled. New Engine reopens the catalog in-process; this is not process restart evidence.
