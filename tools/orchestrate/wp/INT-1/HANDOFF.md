# INT-1 integration

Local branch `local/rut-build`, starting at `afe97d4c`.
Cherry-picked `46b1bf54..6a4cedcc` (all four LR-11 commits) without squashing:
`86734712`, `7d5bf95e`, `92243f2c`, `97176222`.

## Conflict resolution

The only textual conflict was `crates/pipeline-cpu/src/render.rs`.
Preserved LR-10's host local-mask hook and LR-11's Point Color split: both the
pre-monochrome Point Color group and the remaining local adjustments use the
host hook when supplied, otherwise the CPU mask compositor. Locals remain before
Upright, and the existing retouch-before-Detail/Tone sequence is unchanged.
LR-10's embedded DNG profile and ACR3 path are unchanged, including HueSatMap,
exposure, LookTable, and profile tone ordering.

Clean merges preserve the schema-predicate union, translation-matrix rows,
combined mask/depth resolvers, offline-proxy outcome, LR-9 no-op policy table,
and LR-7 single Import history. Existing import/pixel goldens are unchanged.
Cargo.lock is unchanged by INT-1; relative to `46b1bf54` it has only the two
approved raw-decode edges (`jxl-oxide`, `zune-jpeg 0.5.15`). No board.json changes.

## Combined synthetic regression

`lrcat::combined_tests::int1_offline_proxy_nested_locals_adobe_render_and_orientation`
applies an invented offline Smart Preview row through FFI, with nested gradient
mask, local curve and Point Color, monochrome off, and saved Upright enabled.
It verifies one Import history entry, schema 4, structured diagnostics through
`diagnostics::entries()`, stored proxy orientation, and CPU Adobe rendering.
Embedded profile dispatch must match an explicitly supplied DNG profile; removing
either local operator or Upright must change pixels. The source is reopened
through the production proxy/recipe path, with swapped non-square extents and
consumed presentation orientation.

## Verification

After `cargo clean -p` for all ten touched crates:

- `cargo test --locked --release` for raw-decode, pipeline-adobe, pipeline-cpu,
  pipeline-gpu, compositor, filters, image-core, import-lrcat, engine-api, sidecar,
  merge, export, previews, mask-store, tessera-mcp, tessera-ffi: PASS. No command
  filters/exclusions. 2,553 parent-suite tests plus three child-process executions
  passed, zero failed; 89 suite-declared ignored. All existing goldens passed.
- `cargo clippy --locked --release --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cd apps/mac && ./build-ffi.sh`: PASS. Regenerated bindings differ only in
  whitespace; that churn was discarded.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**. 920 XCTest tests, three
  skipped, zero failures; all five Swift Testing tests passed.
- `cd apps/mac && swift build -c release --product Tessera -Xswiftc
  -strict-concurrency=complete -Xswiftc -warnings-as-errors`: PASS (147.79 s).
  Existing linker warning: `blake3_neon.o` targets macOS 26.2 while the app links
  for 15.0. No Swift compiler warnings/errors; macOS 15 runtime compatibility
  was not tested.

Test commit: `5ce96b45`. Gate logs are local at `/tmp/int1-release-tests.log`,
`/tmp/int1-clippy.log`, `/tmp/int1-fmt.log`, `/tmp/int1-build-ffi.log`,
`/tmp/int1-swift-gate.log`, and `/tmp/int1-swift-release.log`.
All checks use `$HOME/.cache/tessera-target/LR-8`, six Cargo jobs and six Rayon
threads. Only synthetic catalogs and existing repository fixtures were used.
No GUI launch, real catalog access, or write under ~/Pictures. No push.
