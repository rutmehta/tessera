# WP M2-03 — engine-api contract revision 1.1 (Opus)

Inputs: crates/engine-api/CONTRACTS.md, crates/sidecar (merged; read src/develop.rs and the crs handling) and tools/orchestrate/wp/M1-03/FINDINGS.md (sidecar author's evidence on Adobe XMP), crates/import-lrcat if present.
Revise the contracts, bumping CONTRACT_VERSION to 1.1.0 and RECIPE_SCHEMA_VERSION if the serialized default changes, with the change log updated:
1. `CrsKey`: move the Enhance* keys to an `aux:` namespace (add a `namespace` field to the key table: Crs | Aux | Xmp), fix `EnhanceDenoiseLumAmount` → `EnhanceDenoiseLumaAmount`, mark "already applied" keys as informational (no recipe_path, importer records them in provenance). Re-verify HDREditMode, HDRMaxValue, LensProfileSetup, PostCropVignetteStyle against the sidecar findings and ExifTool's XMP2.pl table (fetch it) and correct spellings/types.
2. Lens profile and camera profile identity: replace the single profile ID with a struct that carries `{ name, filename, digest, setup }` for lens and `{ name, digest }` for camera so XMP fields round-trip losslessly.
3. `ProcessVersion`: define the native→Adobe export policy: `crs_value()` for native returns the PV6 string plus a `ts:NativeRevision` companion property, and document that exported native recipes are "best-effort PV6".
4. Selection XMP mapping: record in CONTRACTS.md the pick/reject encoding the sidecar crate established (`xmpDM:pick` 1/0/-1, `xmpDM:good`, rating -1 for reject).
5. Any additive fields the importer/sidecar authors requested in their FINDINGS.
Update sidecar to compile against the new contract (minimal edits) and keep all tests green: `cargo test -p engine-api -p sidecar --release`, clippy -D warnings, fmt. Commit as "wp(M2-03): engine-api 1.1". Report the full change list.
