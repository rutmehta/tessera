# Export integration checkpoint

First resolved tree: `9fe7a245e2bc070e65fb3f4c0a4301f233ee473d`, committed as `5e5593c`.
This is the bounded DNG 1.6/original embedding, PQ/HLG PNG/AVIF and native
EXIF/IPTC/XMP metadata slice from `wp/M2-45d` at `69bcd3e`. Gain-map JPEG remains
separate and is not delivered by this change.

The first Rust run stopped because the isolated worktree lacked ignored RAW
fixtures. Its raw failure log is preserved. The existing main fixture directory
was linked, SHA-256 recorded, and the full command rerun without source edits.

- Five-package release Rust tests: **564 passed, 0 failed, 21 ignored**.
- Strict all-target Clippy on export, sidecar, CLI, FFI, MCP: pass.
- Locked workspace check: pass.
- Formatting and license checks: pass; existing unused license allowance warnings retained.
- Fresh macOS FFI build/generation and Swift build: pass.
- Existing vendored LibRaw warnings and archive macOS deployment warning retained.

Current main was merged afterward as `d11c523`; Rust/Cargo/CLI files are byte
identical to the gated tree and Swift Sources/Tests match current main at that
checkpoint. Final combined Swift regression validation is pending; this report
will be completed before main integration.

## Final combined validation

Reviewed ownership/save-order commits e886888 and c38a382 were merged with the
export slice and current UI baseline. Combined source: **b82fe3330d01d5011b779bddb2555f5702befe89**.
The initial full Swift run executed431tests with one existing skip and one real
failure: the old export test still expected DNG1.4. The approved writer declares
DNG1.6 and retains DNGBackwardVersion1.4. The test now verifies both exact tags;
its focused regression passed before the final full run. The original failure
log is retained; no product behavior or acceptance threshold was relaxed.

The final full Swift release gate passed **436 XCTest cases, one existing skip,
zero failures, plus5 Swift Testing cases**. It includes all23 ownership regressions,
the DNG contract correction, and existing UI/Document/Develop tests. The source
and generated FFI/archive hashes are recorded in final-source.json. Root reviewed
the ownership barriers, mutation resource identity, publication guards and tests.

This checkpoint does not include UX02a navigation or gain-map JPEG. M5-31 cold
performance remains unaccepted after B’s106.084084ms first-sample failure.
