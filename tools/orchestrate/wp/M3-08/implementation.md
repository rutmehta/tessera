# M3-08 implementation record

Spec: `tools/orchestrate/wp/M3-08/brief.md` and docs/10 sections 2–3.

Architecture: synchronous Console owns the catalog, resolves image IDs to sidecars,
and calls underlying crates directly. MCP serializes access to that console. Stdio
uses rmcp; CLI replaces itself with the sibling/PATH MCP executable.

Execution: native, in this worktree, no commits. All Cargo commands use the supplied
external target directory. No engine-api changes.

Vertical slices:
1. Failing fixture test → persistent Agent tone edit → rendered histogram.
2. Failing mutation tests → masks/crop/presets/selection, validation and conflicts.
3. Failing comparison/catalog/export tests → read tools and engine integration.
4. Failing transport tests → derived schemas, rmcp resources and stdio; CLI exec.
5. Required release tests, clippy and formatting; document missing engine fields.

Review focus: malformed requests must fail before mutations; stale recipe hashes
must preserve sidecars; style IDs must not become arbitrary paths; unsupported
operations must never claim success; protocol stdout must contain only JSON-RPC.

Dependency evidence: upstream rust-sdk workspace manifest declares Apache-2.0:
https://github.com/modelcontextprotocol/rust-sdk/blob/main/Cargo.toml
rmcp inherits that license. Cargo network resolution was initially unavailable.

## Completed verification

The required command sequence completed successfully with
`CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-08` on every command:

- `cargo test -p tessera-mcp -p tessera-cli --release`: 34 tests passed.
- `cargo clippy -p tessera-mcp -p tessera-cli --all-targets -- -D warnings`: passed.
- `cargo fmt --check`: passed.

The temporary offline console-check harness was removed after full SDK tests
passed. Red/green slices covered persistent tone/history/histograms, mutations,
comparison/catalog/export, CLI exec, and in-memory/spawned stdio protocol behavior.
The existing CLI model audit test now selects only its bundled convolution model
from the production manifest, preserving its real hash and CoreML assertions
without downloading unrelated production models.

`crates/tessera-mcp/README.md` documents usage, StyleId lookup, resource URIs, and
engine contract gaps. In particular, non-develop actions have audit-only history,
the typed comparison output lacks image/metric fields, and unsupported engine
options return explicit errors. No engine-api files were changed. No commits made.

Independent parent verification reran the exact chained test/clippy/fmt command
successfully (exit 0), with 34 passing tests and the external target directory
preserved. `git diff --check` passed. All changed/untracked paths are within the
work package allowlist. `cargo info rmcp` independently confirmed Apache-2.0.
The native LibRaw dependency emits existing C++ warnings, but the Rust clippy
gate with `-D warnings` passes.
