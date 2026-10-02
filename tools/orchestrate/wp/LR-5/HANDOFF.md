# LR-5 — AI masks, Machine B

Branch `wp/LR-5-ai-masks`, local only. Base
`4ecf521de9583f8bf3d962ec6ff4cbaa72591004` includes LR-4 through LR-4e and
`origin/main` `486d069f` (including LR-SCHEMA, LR-DIAG and LR-7). The required
`e37de957` target is already an ancestor; no rebase or sibling merge was needed.
`origin/wp/LR-6-lens-blur` was inspected read-only for the LR-6e storage pattern.

## Commits and evidence

- RED `9f73a8389c26fbff461f115d03c5e6cc3f68c0d2` — `test(LR-5): require AI mask decoding and durable raster pins`.
  Release importer test failed on unsupported `Mask/Image`; mask-store test failed
  to compile because `put_pinned` / `remove_pinned` did not exist.
- GREEN `dc8df0a82d23a90ad463a55dadd1b87494ec6852` — `feat(LR-5): import AI mask resources with bounded pins and regeneration`.
- The following `docs(LR-5):` commit records the final evidence and matrix rows.

## Implementation

`MaskComponent.adobe_ai` is one additive optional recipe object, absent by default
and omitted when absent. It carries category, opaque resource ID, optional store
key and regeneration state. The `adobe_ai_mask` v4 predicate covers the object,
including disabled/history trees, with the shared bumped-only-when-present test.
`RECIPE_SCHEMA_VERSION` remains 3; no first-lane checklist changes were duplicated.
The MCP schema extractor includes the new nested type; no dependency was added.

The sidecar codec decodes recognized `Mask/Image` subtypes and explicit AI kinds;
import-lrcat audits the entire parent, retains exact Lua/XMP source and calls the
shared diagnostics helper. All new matrix mappings are `approximate`. Person
sub-parts map to subject; each part's reason states that regeneration cannot
isolate it. Objects require a box or reference-point prompt. Numeric subtype 0
with a prompt is an approximate object interpretation, not verified Adobe parity.
Unknown/malformed forms remain untranslated. Nested, disabled, inversion and
ordered add/subtract/intersect semantics remain LR-4's.

`LrcatImport::apply_with_mask_resolver` accepts an optional caller-owned opaque-ID
resolver. It resolves only during APPLY through `ml_segment::MaskStore`, which
already re-exports mask-store. Resolved full sensor-aligned grayscale PNG/TIFF
rasters are stored before recipe publication. IDs never become paths, and the
resolver must expand Adobe crop/origin data before returning bytes. No proprietary
helper is opened by this lane. Ordinary `apply` leaves regeneration pending.

Pins live at `<Tessera support>/imported-masks/pinned`, keyed by stable image ID
and compact resolved-raster slot. The per-image bound is **256 rasters and 256 MiB
total including headers/checksums**. Size and grayscale/extent checks precede
raster allocation; excess resources remain pending. Re-import replaces slots and
removes obsolete pins. Failure before publication restores prior usable slots.
Atomic replacement uses one extra raster-sized temporary file per writer.
`Engine::forget_missing` removes pins only for absent image records and retains
pins when the original still exists. Inference LRU eviction cannot remove pins.

Import attachment updates the existing Import history entry, not a new entry.
User edits, JSON and native XMP retain references. Resolution removes only the
exact pending LR-5 regeneration reason through the shared diagnostics module;
other lanes' entries survive. Resume reports read the published recipe so they
do not report already-resolved masks as pending.

The shared compositor can select by component identity. Preview and export read
the stored raster without inference; same-category components can have different
planes. Preview checks the payload checksum to invalidate in-memory alpha after
same-key replacement. Missing/wrong-size imported references fail explicitly.
Export receives the engine support root explicitly, including print rendering.
Regeneration retains the existing subject/sky/background/prompted segmenter seam;
no person-part request or new model was invented. Rendering mutates neither
import diagnostics nor history. Existing GPU admission excludes these AI kinds.

## Gates

**PASS** on the GREEN tree plus the documentation/matrix update:

- Release test command below exited 0: **1,470 top-level test passes**, plus
  three successful child-process test runs; **53 existing ignored tests**,
  zero failures. The child-process harnesses report 21 internal filtered tests;
  the command supplied no filters, skips or exclusions.
- Clippy, release, all targets, `-D warnings`: passed (13.05 s final run).
  LibRaw's existing C/C++ compiler warnings were emitted by its build script;
  no Rust/clippy warnings remained.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Shared translation-matrix guard and all of its negative tests: passed.
- Unchanged B5-29c full retained-source golden, LR-4 compatibility/retained byte
  pins, streaming equivalence and schema tests: passed; no golden was re-pinned.
- Import streaming memory/time gate and FFI 20,000-image streaming memory gate:
  passed; the latter test finished in 113.37 s. No latency-flake rerun was needed.
- FFI LR-5 end-to-end apply/replace/rollback/resume/forget/export, injected
  category regeneration, 256-slot overflow, corrupt/wrong-size resource fallback,
  same-category distinct-plane rendering and same-key preview invalidation:
  passed. JSON/native-XMP round trips and conditional schema test passed.

Early RED failures and intermediate compile/matrix issues were resolved; no tests
were weakened. In particular, the matrix guard caught explicit AI categories
still carrying the legacy warning; fixing their metadata initialization removed
that warning through the shared approximation path.

Environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-4-parametric-masks"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo test --release -p import-lrcat -p engine-api -p mask-store -p sidecar -p mask-ai -p export -p tessera-ffi -p tessera-mcp -p ml-depth -p pipeline-cpu -p pipeline-gpu
cargo clippy --release -p import-lrcat -p engine-api -p mask-store -p sidecar -p mask-ai -p export -p tessera-ffi -p tessera-mcp -p ml-depth -p pipeline-cpu -p pipeline-gpu --all-targets -- -D warnings
cargo fmt --all --check
```

No command-level exclusions. Repository RAW fixtures are included. Existing
ignored/env-gated model tests remain opt-in; new tests use synthetic rasters and
an injected segmenter. No Swift gate or app launch was performed under the
Machine B override.

## 29c compatibility and merge notes

The decode layer performs no raster I/O or inference. Only recognized AI forms
add state/diagnostics; exact source remains for all approximate translations.
No ordinary field default, recipe format constant, Cargo manifest, Cargo.lock,
board.json or real catalog was changed. `LR-RULINGS-FROM-A.md` remains untracked
and must not be committed. Coordination is this handoff only; nothing was pushed.

LR-6e independently adds the same mask-store pin API. When the coordinator lands
LR-6 first, reconcile `MAX_PINNED_BYTES`, `put_pinned`, `remove_pinned` and pinned
`get` once; retain LR-5's `pinned_revision` invalidation support. Both FFI resolver
entry points need composition at merge; this lane deliberately did not merge or
copy the LR-6 branch. Combine both removal hooks in `forget_missing`. The
coordinator owns final generated Swift bindings for the added resolver callback.

## Evidence limits

Adobe's catalog FAQ identifies `.lrcat-data` as AI-edit storage, distinct from
`.lrdata` previews. Public first-hand XMP examples establish descriptions and
opaque digests, not a documented binary decoder or table association. Links and
supported forms are in `crates/import-lrcat/README.md`. Automatic Adobe blob
association/decoding remains unavailable; the caller must supply decoded bytes.
Person sub-part inference is unavailable. Object descriptions without usable
prompts and unknown subtypes remain retained. No Adobe-rendered synthetic chart
or public DNG+XMP pixel-parity comparison was used, so no new mapping is claimed
as exact. All fixtures and raster values added here are invented.
