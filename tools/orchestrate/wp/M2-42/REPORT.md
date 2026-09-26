# M2-42 — People FFI follow-ups

## Delivered

- `PersonInfo`: `named`, `confirmed_count`, `face_count`, `medoid_face`. Counts retain the existing active-queue scope (`face_count == faces`). The medoid resolves the indexed descriptor to an actual catalog member, independently of the sharpest cover and queue subset. Missing/invalidated descriptors return `None`. Identical descriptors use stable image/ordinal order. Matching reproduces the normalization used by clustering/repair, not an epsilon-based nearest-face guess.
- `PeopleJobResult.sample_size`: eligible pending training rows bounded by the clustering implementation's 1024-entry reservoir. Exact jobs report their actual eligible training count; an incremental no-op reports zero.
- `person_members(person_id)`: all catalog members in stable image-ID/ordinal order, without clustering, queue filtering, or pagination truncation. Unknown/empty identities return an empty list.
- `undo_people_edit()` / `redo_people_edit()`: return the applied edit's menu description, or `None` for empty history. Separate session-local history, bounded to 32 edits. Assign, confirm/unconfirm, merge, split, and name are reversible. Merge undo restores the source ID, name, members, confirmations, and medoid. New changes clear redo; no-ops and failed edits do not consume history.
- Index inverses operate transactionally on affected identities, not the whole catalog. Unrelated identities survive. Intervening affected-row changes or changed detections reject replay without consuming history; automatic medoid-only repair does not block replay.
- Naming history includes library-document bytes and, only when opted in, sidecar bytes. Undo restores prior keywords/XML exactly and removes newly created sidecars. Replay rejects intervening file edits, compensates ordinary file-write failures, and reports compensation failures. SQLite plus files are not crash-atomic.
- Manual edits/replay no longer implicitly trigger clustering on the next read. Explicit refresh remains available.
- UniFFI Swift/C bindings regenerated. New record fields have UniFFI defaults, verified by compiling old-style Swift initializers. `TesseraCore/Assist/People.swift` was not modified; the existing consumer remains source-compatible. Switching its loading/UI behavior to the new APIs is not part of this FFI-only change.

## Verification

Ran the exact required gate on the final code with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-42` throughout:

```sh
cargo test -p tessera-ffi --release --lib --test assist && cargo clippy -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing --filter People)
```

Exit 0. Evidence: `tools/orchestrate/wp/M2-42/gate.log`.

- Rust FFI: 40 unit tests and 12 assist integration tests passed.
- Clippy with warnings denied: passed.
- Workspace formatting check: passed.
- Scope audit: all 10 changed/untracked files are allowed; no checkout-local `target/`. `git diff --check` flags trailing whitespace emitted by UniFFI in the generated Swift/C bindings only; generated output was left verbatim.
- Generated FFI archive/bindings, Swift debug build, and release People tests: passed (11 XCTest cases, no failures).
- Additional `cargo test -q -p index --release --test people`: 8 passed, 1 existing ignored benchmark.
- New coverage includes a 1025-face sampled job, exact/no-op job sample counts, medoid versus sharpness and nearly identical descriptors, member lookup, complete edit undo/redo, source-ID restoration, confirmation restoration, unassigned faces, name/XMP restoration, per-session isolation, redo branching, bounded history, external conflicts, and re-detection.
- Test-first runs observed the missing fields/methods fail before implementation. A targeted medoid regression also failed against epsilon matching before the exact-normalization fix.

Non-fatal build output includes LibRaw C/C++ warnings and a linker warning that the cached `blake3_neon.o` targets macOS 26.5 while the app links for macOS 15.0. No UI/manual acceptance walkthrough or model download was performed. No commits or pushes were made.

RESULT: PASS
