# B5-42c — Accessibility audit and linear row lookup

Branch: `wp/B5-42c`, based on `origin/main` at `87536669`. Local only.

## Changes

- Restored the audit's role-independent press criterion using modern `accessibilityPerformPress` selector support and `isAccessibilitySelectorAllowed`. AppKit exposes inherited no-op implementations on passive NSView/NSCell objects; the test helper compares Objective-C implementations to exclude only those base stubs, without executing an action. Custom NSObject and NSView pressable groups are audited for the same identifier/name requirements as standard interactive roles. Passive views/text fields are regression-covered.
- Added row-position assertions comparing both `accessibilityIndex` and `document.layers.row.N` to `outline.row(for:)`. The existing hosted Layers action test checks metadata before and after disclosure. A lightweight native outline fixture realizes 321 rows, collapses a populated 20-child group, re-queries accessibility children, and verifies all 301 remaining positions and the final item's move from 320 to 300.
- Replaced per-child NSArray linear search with one per-query NSMapTable index using strong object-personality keys (`hash`/`isEqual`). First-match semantics, native order, metadata, realized-row replacement, fallback behavior, and non-row children remain unchanged. This preserves value matching across AppKit's recreated proxies while making row mapping expected O(n).
- Marked the hosted outline finder `@MainActor`, removing its isolation warning.

## Tests first and validation

Tests-first commit: `9ed9642c`. Implementation commit: `7188b606`.

The initial tests ran against the original production lookup. The large-outline mapping test passed; the expanded audit exposed inherited no-op AppKit methods and the document fixture's empty group. The final audit handles actual offered actions, and the populated native fixture verifies the collapse position shift. No timeout or unrelated test changes were made.

Focused final run:

```sh
cd apps/mac
swift test -c release -Xswiftc -enable-testing --filter DocumentAccessibilityTests
```

Passed: **7 tests, 0 failures**, 20.195 seconds; **72 audit scenarios / 1,201 interactive observations**. Large-outline mapping test: **0.066 seconds**. One earlier focused run hit the existing toolbar two-second layout-settle timeout; an unchanged rerun and the full gate passed.

Required gate:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-42c
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

Exit **0**:

```text
Build complete! (73.78s)
Executed 917 tests, with 3 tests skipped and 0 failures (0 unexpected) in 410.267 (410.426) seconds
✔ Test run with 5 tests in 2 suites passed after 0.049 seconds.
SWIFT GATE OK
```

Required strict release build:

```sh
cd apps/mac
swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

Exit **0**, with **0 warnings and 0 errors** in the captured Swift build output:

```text
Build of product 'Tessera' complete! (202.61s)
```

`git diff --check` is clean. Final source was validated by both gates before this documentation-only commit.

The FFI build reports existing vendor LibRaw warnings. No Rust, Cargo.lock, or board.json changes. No foreground GUI launch; only builds and background/inactive test harnesses. Commits carry the requested co-author trailer. Nothing pushed or merged.
