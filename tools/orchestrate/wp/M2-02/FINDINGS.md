# M2-02 handoff

Status: partial implementation; not acceptance-complete.

Implemented and exercised:

- RecipeDocument deserialization routes through Recipe::from_json, applying schema upgrades without changing synchronization metadata.
- ts:ExportHash gates NativeRevision; changed/unknown CRS values invalidate it. Re-export cannot reactivate a stale native companion. Prefix aliases are resolved by namespace.
- Shared sidecar recipe translation now powers import-lrcat, with catalog fallback and diagnostic/source-retention behavior retained.
- Native round trip for nonempty PointColors, LensBlur, all retouch variants, every mask kind/model reference, local IDs/combines/inversion/overlay, and all mapped scalar/curve/Look fields.
- Exact native curve precision is retained beside Adobe's integer curve points, with visible-point checks to respect external edits. Native lens profile variants have a hash-gated companion.
- Proptest generates nondefault DevelopSettings and checks every contract Field pointer after fresh XMP export/import and re-export. Deterministic structured tests exercise all mask and retouch variants. Regressions found and fixed curve quantization, empty Look identity, CR normalization, lost native lens source variants, and stale-companion reactivation.

Acceptance gaps:

The new granular ts RDF preserves native structures but is not a complete Adobe payload translator. Adobe PointColors string grammar, brush Dabs, colour range payload/type codes, native retouch-to-Adobe healing payloads, and LensBlur FocalRange/BokehShape are unresolved. Unsupported foreign payloads still warn and retain source rather than invent conversions. LookSettings only stores style and amount, so authoring Look tables needs a reviewed resource/model contract. No engine-api changes made, and no Lightroom rendering/interoperability test occurred.

See `crates/sidecar/UNMAPPED.md` for field-by-field unmapped reasons, private-companion rules, and interoperability boundaries. Do not interpret native self-round-trip success as closing the Adobe export gaps.

Verification rerun directly in the M2-02 retry, exit status 0:

    cargo test -p sidecar -p import-lrcat --release && cargo clippy -p sidecar -p import-lrcat --all-targets -- -D warnings && cargo fmt --check

CARGO_TARGET_DIR remained `/Users/rutmehta/.cache/tessera-target/M2-02`. No commits or pushes.

Retry assessment: green checks establish native codec regressions, not full Adobe
export acceptance. `crates/engine-api/src/recipe/settings.rs:435` defines
`LookSettings` with only `style` and `amount`; it cannot supply Look tables.
No engine-api changes were made. Completing Look table authoring requires an
approved model extension or an external style-resource resolution contract.
The remaining Adobe payload translations above also remain incomplete.

RESULT: FAIL Adobe structured export remains incomplete; Look table authoring requires a model or resource contract.

## Latest verification

Re-read contract 1.1, the shared importer, structured encoder, property test,
and `LookSettings` in this run. Ran the exact required test/clippy/fmt chain
again with the external `CARGO_TARGET_DIR`; the entire chain returned exit 0.
No implementation changes were made in this verification run. The Adobe
translation gaps above remain, so the work package still fails acceptance.
In particular, a style ID cannot supply missing Look tables without a defined
resource resolver or an approved model extension. Engine-api was not modified.
Board orientation was unavailable: no `HERMES_KANBAN_TASK` was supplied, and
`kanban_show()` returned a missing-task-id error. No board state was changed.
