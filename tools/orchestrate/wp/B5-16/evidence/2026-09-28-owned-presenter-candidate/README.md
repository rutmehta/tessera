# Owned document-save presenter candidate validation (2026-09-28)

This package contains source/build evidence only; it contains no app bundle and no native archive binary. The exact source checkout remains `codex/document-owned-native-validation` at `d695e6a12b5f6753bc1da6c63bd1568d2ef20ab7`, derived from `93244f8f` plus `origin/main` `379ae615`. The parent baseline branch remains at `010617b835be756495a7ca917a5b673c467a3401`.

The preserved `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03` archive copy is at `/Volumes/betterSSD/tessera-validation/document-save-settlement/candidate-62d546dc-4a45-20260928/libtessera_ffi-8ab43f64-preserved.a`. Candidate tests used `4a45f8235a8c6382d0dad9be735d5cd336a86c20eb50c15051b1c55654444596`, from `/Volumes/betterSSD/tessera-validation/recipe-xmp-parity/ffi-4a45f823/ffi/libtessera_ffi.a`; that archive was installed in the checkout's ignored `apps/mac/build/ffi/libtessera_ffi.a` during both successful runs. Generated Swift/header/modulemap hashes were unchanged.

## Attempt 1: compile/setup failure, preserved

At source commit `62d546dc0c10cba8b901d23a7ba7fdb9eae29ab8`, the focused suites did not execute. Swift compilation found the migrated test fixture was not marked `@MainActor` and an existing test passed `SaveAsRequest` where `UUID` was required. Direct child exit was 1; watchdog did not fire. This was not a behavioral result. Both compiler diagnostics and the unmodified attempt source/hash state are preserved in `compile-attempt-62d/`.

The follow-up test-only correction is commit `d695e6a12b5f6753bc1da6c63bd1568d2ef20ab7`; it adds `@MainActor` to the helper fixture and passes `old.id` to cancellation. No product source was changed by that correction.

## Focused GREEN

The exact filter was `DocumentSave|DocumentLoad|DocumentLayersActivation|DocumentAdjustmentAnalysis|DevelopRecovery`, Release, `--jobs 2`, `-Xswiftc -enable-testing`, using the isolated betterSSD scratch at `/Volumes/betterSSD/tessera-validation/document-save-settlement/candidate-62d546dc-focused-4a45-20260928/scratch`.

Result: 90 XCTest executed, zero failures; direct child exit 0 and no timeout. The new headless legacy Save As test and real attached-NSWindow bridge replacement/stale-dismantle test both passed. Raw log and pre/post source/archive manifests are in `focused-green-d695/`.

## Full Release suite

The full Release suite used the same frozen commit, archive, and scratch. Result: 642 XCTest executed, 1 skipped, zero failures; 5 Swift Testing tests passed; direct child exit 0 and no timeout. Raw log and pre/post source/archive manifests are in `full-green-d695/`.

The tracked source HEAD and tracked source/generated-file hashes remained stable through the focused and full runs. The FFI archive was an ignored build input and is recorded separately in `archive-provenance/`; the old `8ab` copy was preserved before replacing that input. This evidence covers Swift tests, not the native queued-sheet AppKit probe or GUI acceptance. Root owns main integration.
