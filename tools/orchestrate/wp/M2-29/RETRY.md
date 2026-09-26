# M2-29 retry verification

Result: FAIL, blocked by the unchanged path allowlist.

The exact requested command was executed in this retry with the inherited `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-29`:

`cargo test -p image-core -p pipeline-gpu -p tessera-ffi -p export --release && cargo clippy -p image-core -p pipeline-gpu -p tessera-ffi -p export --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build)`

It returned exit 101 at `crates/tessera-ffi/tests/develop.rs:501`, in `jpeg_images_are_refused`. That test explicitly requires JPEG opening to fail, contradicting this work package's required behavior. The new `develop::tests::jpeg_develop_session_renders_nonblack` passed, as did the image-core RGB decoder tests and resident GPU RGB tone comparison. The obsolete refusal test was not disabled or special-cased. Its path is outside the permitted edits.

The `&&` chain stopped before clippy, formatting and the Swift build. Clippy and formatting were subsequently run independently with exactly the requested flags and passed. `git diff --check` also passed. Swift was not rerun in this retry; REPORT.md describes the prior attempt's build result.

Read-only inspection also reconfirmed that `crates/tessera-mcp/src/pixels.rs:24-50` still decodes RGB through its independent sRGB-only conversion and its extension gate omits HEIC. `crates/tessera-mcp/src/preview.rs` uses that decoder. Those files cannot be updated under the current allowlist, so complete agent/MCP parity remains blocked.

Required scope expansion: permit `crates/tessera-ffi/tests/develop.rs` to replace the obsolete refusal assertion with successful rendering coverage, and permit the MCP decoder/preview integration and associated manifest/tests. No production changes were made in this retry, no tests were bypassed, and no commits were created.

No Kanban task ID was available: the required initial `kanban_show()` returned `task_id is required (or set HERMES_KANBAN_TASK in the env)`. This report is the durable handoff rather than an ungrounded board transition.
