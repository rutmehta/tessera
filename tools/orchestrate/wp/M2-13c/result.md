# M2-13c verification

## Change

Enabled `lcms2 = { version = "6", features = ["static"] }` in the workspace dependency, bundling vendored LittleCMS in the Rust archive used by Swift. All consumers already inherit the workspace dependency, so no per-crate override or lockfile update was needed. Package.swift and CI remain unchanged: no extra system package installation or linker flags.

The existing concurrency fix at DevelopPanelsTests.swift:288-298 is retained. The detached task captures the generated Sendable DevelopSession and scalar surface ID/width/height, not the non-Sendable IOSurface. The main-actor caller keeps the IOSurface alive through the awaited render with withExtendedLifetime in defer. No new unsafe concurrency annotations were added.

## Toolchain

`xcrun swift --version`:

    swift-driver version: 1.148.6 Apple Swift version 6.3.3 (swiftlang-6.3.3.1.3 clang-2100.1.1.101)
    Target: arm64-apple-macosx26.0

Only this local toolchain was exercised; remote CI / its older Swift toolchain was not run here.

## Verification

- Baseline `swift test -c release --filter DevelopPanelsSessionTests` failed to link with undefined `_cms*` symbols (baseline.log).
- Ran the exact requested command with CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-13c throughout:

      cd apps/mac && ./build-ffi.sh && swift build -c release && swift test -c release && swift test

- First post-fix run: release passed; debug had one failure at BridgeTests.swift:50 (thumbnail cache XCTAssertNotNil). DevelopPanelsSessionTests passed in both configurations. Preserved in verification.log.
- Repeated the entire command without further code changes: exit 0. Release and debug each passed 25 XCTest tests and 5 Swift Testing tests with zero failures (verification-rerun.log). The intermittent thumbnail cache assertion is not changed by this scoped fix.
- `cargo tree --locked -p tessera-ffi -e features -i lcms2-sys` confirms lcms2/static enables lcms2-sys/static.
- `otool -L` on the rebuilt Rust dylib shows no LittleCMS dynamic dependency.
- `nm -gU` confirms native definitions of `_cmsCreate_sRGBProfile` and `_cmsCreate_sRGBProfileTHR` in libtessera_ffi.a. Apple's nm cannot decode some unrelated newer Rust LLVM objects and exits 1, so this is a positive symbol observation, not a claim of a clean full-archive nm scan.
- `git diff --check` passes. The coordinator's pre-existing brief.md modification was preserved. No commits created, no files outside the allowed worktree modified (apart from the explicitly required external Cargo build cache).

RESULT: PASS
