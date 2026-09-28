# Develop patch-retention evidence (2026-09-27)

This evidence covers the source-only Swift change that retains a rejected coalesced Develop settings patch for a later explicit retry. The tests also cover newer reentrant values, final-vs-interactive mode, failure-callback updates, deferred flush behavior, and synchronous host callbacks. It does not validate close-failure recovery or current-main/current-FFI integration.

The implementation commit is `96fac8193006dca5a39127cbf498c0ca182b59d1`. Final tested source hashes are recorded in `final-pass/freeze-final-forwarding.sha256`: `DevelopController.swift` is `72b64733a5c5bf42abfa32bcd9c809a88b495e7430d166a870c4f0cfc15cb7f0`; `AgentReviewOwnershipTests.swift` is `05caa0f5a3be8fe5d3f1ae83a84d2c8462bfabe37dea7c9df66f83bcbada84a5`.

## Results and preserved failures

- `original-red/`: test-first rejected-patch regression on base `435211b2`; direct exit 1, one test with five expected failures showing pending exposure was lost after `setSettings` rejection.
- `prior-green/`: earlier basic retention attempt, direct exit 0 for 33 tests. This predates the reentrancy/scheduler cases and is not final validation.
- `nested-red/`: test-first nested flush regression; direct exit 1. It showed the older in-flight patch could be overtaken and later overwrite a newer value.
- `first-final-attempt/`: an initial final-candidate run hung because several test doubles signaled their blocking close semaphore only in `defer`, after awaiting close. The sampled main-thread stack and interrupted log are retained. Test setup was corrected to signal before close.
- `second-final-attempt/`: corrected semaphore setup exposed a SIGSEGV in the test double. The crash stack reached `DevelopController.didRender` → `DevelopSession.getHistogram` through `BlockingCloseSession`, which forwarded listeners to its wrapped native session but had not forwarded `getHistogram`. The test double now forwards that method. This was not identified as a product failure.
- `final-pass/`: exact final source passed the focused Release command below: 39 XCTest executed, 0 failures (36 `AgentReviewOwnershipTests`, 3 `DevelopTests`), direct exit 0. Swift Testing reported zero tests in its separate runner.

Command:

```sh
swift test --jobs 2 --package-path apps/mac \
  --scratch-path /Volumes/betterSSD/tessera-cache/swift/develop-close-be02edf9-red/scratch \
  -c release -Xswiftc -enable-testing \
  --filter 'AgentReviewOwnershipTests|DevelopTests'
```

## FFI provenance and limits

The Swift-only run used the preserved 0a9 current FFI inputs. `libtessera_ffi.a` SHA-256: `0a9b2de3dee742751da067147805715925ac005e7036d0268d36296dc25168ae`; `CTesseraFFI.h`: `e188c20e48663ce3c722a3ae9ee25f66b8412ddff711e55cd897063fef65e703`; generated `TesseraFFI.swift`: `71e2237cfbb132f2a01ecdd17b092a32fc03f2b4239ef116216824bf058f5361`; module map: `efda206de8cf8eb6c092c29fd32f286b9a46d9d7c4c150e6e9b66941dc43d6bd`. The 0a9 archive was also preserved at the external `input-backup/libtessera_ffi.a.0a9`; its hash matches the tested checkout archive. The older archive backup remains separately preserved. No archive binaries are duplicated in this evidence directory.

Root must run the current-main/current-native-FFI integration and full Release suite before accepting the change. No close API, AppModel navigation, or GUI behavior was changed here.
