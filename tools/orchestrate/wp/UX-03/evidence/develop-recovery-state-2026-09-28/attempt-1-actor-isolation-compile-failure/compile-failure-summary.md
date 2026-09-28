# Preserved attempt 1: coordinator actor-isolation compile failure

Frozen checkout HEAD: `a19d79979badc883840dbb8cb0cea230a0bf4a4b`.
Command: `command.txt`.
Raw log: `swift-test-release.log`.
Direct process exit: `direct-exit.txt` = 1; elapsed `83.8` seconds. No XCTest cases ran.

The only errors are in `DevelopRecoveryCoordinator.swift`, outside the test-file ownership:
- line 59: `Gate.finish()` synchronously calls actor-isolated `finishGate` from a nonisolated context.
- line 77: `Record.init` reads actor-isolated `controller.imageID` in a nonisolated initializer.

No AppModel/test source was edited in response. Frozen source/archive/header hashes remain in `source-and-ffi-sha256.txt`; FFI archive SHA-256 is `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03`.
