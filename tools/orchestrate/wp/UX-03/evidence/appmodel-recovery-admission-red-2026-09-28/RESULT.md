# AppModel recovery admission RED (source-only candidate)

The two deterministic `DevelopRecoveryCoordinatorTests` were run in Release against candidate `b928219da34d5bf56b76285ab31927c43d94e31c`, with opener seam commit `58e1b39fc6353745ebb5bc42d531e4fb5a316a18`, merged Core close-result source from `dbb740bb`, and the verified current FFI archive (`8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03`).

Result: 2 tests executed, 2 expected behavioral failures, 0 unexpected failures; direct process exit 1. In each case, the test confirmed the injected close failure was reached, waited for the corresponding AppModel barrier, then observed the opener count become 2 where recovery admission requires it to remain 1. No timeout, crash, or compile error occurred. This is a RED checkpoint; no AppModel recovery implementation is included in this branch.

`swift-test-release.log` is the raw output. `command.txt`, `manifest.json`, `source-and-ffi-sha256.txt`, and the direct-exit/elapsed files identify the frozen run. The tests use generated JPEG fixtures; no GUI or app activation was performed.
