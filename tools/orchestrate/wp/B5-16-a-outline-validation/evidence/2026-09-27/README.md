# A-side bounded outline validation

This evidence belongs to the isolated `codex/document-outline-validation` branch. The
frozen source was commit `f2309eec25e1ce8e3159858d274ece60d906b0fb`, with a clean
working tree. Commit `83dacb25` imported only the four B outline source/test files
from `2429de2`; commits `a78d60f9` and `f2309eec` added A-side deterministic
integration tests and nil-default observation seams. The imported B patch was
`/tmp/tessera-outline-2429de2.patch` (SHA-256
`086a77dd188533f6f27b1f450cd7bb3fbc3918bf2c45922d82939743be7dc3c1`).
`source-manifest.json` records the exact source and ABI-header bytes used by the gate.

The focused gate ran `swift test` in release mode with `--jobs 2`,
`-Xswiftc -enable-testing`, an external Swift scratch directory, and filter
`DocumentOutlineLifecycleTests|LatestRequestBufferTests|DocumentOverlayAnimationTests`.
The outer watchdog was 900 seconds. See `swift-focused.log`, `.exit`, and `.seconds`.
It exited 0 after 170 seconds: 12 selected tests passed (3 blocked-fetch outline
lifecycle, 4 request-buffer, 5 overlay animation), 0 failed. The blocked-fetch
tests exercise document switch with multiple superseding requests, selection clear,
and document close, asserting worker admission and final publication.

The linked retained FFI archive had SHA-256
`19f9f5487752e2588c6febe99d126905cad7d9986285d2aebf633dfaeba2b427`;
its header matched this branch byte-for-byte. That archive predates the latest
Rust resource fixes. This gate validates the scoped Swift outline/timer behavior;
it does not validate current Rust resource changes, GUI rendering, timer cadence,
or process memory. No user catalog, large document, network service, or GUI was used.
