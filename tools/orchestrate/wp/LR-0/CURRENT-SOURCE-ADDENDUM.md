# LR-0 current-source addendum — 2026-10-01

**Scope:** source-only reconciliation of LR-0 publication `5bce045e` against main `2164d370`. Preserve the original matrix (`docs/coordination/LR-TRANSLATION-MATRIX.md`), HANDOFF and INVENTORY-CHECK.json as observations at `68264c74`; do not rewrite their historical counts. This addendum supersedes those counts/statuses only for current-source planning. No product edits, catalogs, compilation, tests or benchmarks. External runtime remains owned by Claude A. Required Claude coordinator approval of LR-0 is still unverified.

## Independently recounted finite inventory

I extracted every complete KEY_MAP tuple, including multiline tuples with trailing commas, and every `crs_keys!` declaration from `git show 2164d370:<path>`, then compared sets to the committed `5bce045e` INVENTORY-CHECK.json. This is a source-text check, not a runtime parser test.

| Measure | Original snapshot | Current source |
|---|---:|---:|
| Explicit Lua mappings / unique Lua names | 199 | **201 / 201** |
| CrsKey declarations / unique names | 143 | **143 / 143** |
| CrsKey names missing from Lua map | 0 | **0** |
| Lua passthrough names without CrsKey | 56 | **58** |
| CrsTarget Field / Legacy / Informational | 133 / 2 / 8 | **133 / 2 / 8** |

Sources: `crates/import-lrcat/src/lua_develop.rs:48-285`; `crates/engine-api/src/recipe/crs.rs:196-373`, both at `2164d370`. The old 56-name passthrough set is wholly preserved; the only additions are below. A mapped Field remains evidence of a target, not Adobe payload support or rendered parity.

## Matrix status amendments

- **UprightFourSegmentsCount** (`lua_develop.rs:274`) and **UprightTransformCount** (`:276`): move these two exact names from the original matrix's unenumerated `Upright*Count` family to explicit **LR-7, retained / untranslated CRS passthrough**. They no longer generate the unknown-Lua-key classification; valid emitted values still meet the XMP adapter's unsupported-property branch (`xmp.rs:132-134`) because no CrsKey exists. Nil is retained even though Lua-to-XMP emission skips it. They have no new typed recipe field or homography/count semantics. Do not invent further suffixes or call this render translation. Source regression: `tests/followups.rs:50-74`.
- **ExtendedToneCurveName2012** plus **ExtendedToneCurvePV2012{,Red,Green,Blue}**: the current adapter explicitly recognizes these five names outside KEY_MAP (`lua_develop.rs:291-297,820-844`); they do **not** increase 201/58. All retain source, name-only/identity cases suppress extended-curve noise, and nonidentity curves receive one named unsupported-HDR limitation (`:300,777-799`; `xmp.rs:158-168`). Add the exact Name key to the LR-2 inventory alongside its four curves. No extended-domain curve field/operator was added; identity diagnostic suppression is not translation.
- **All current unmapped properties and selected advanced families:** per-property retention now exists independently of warnings. The tagged `recipe.unknown["lrcat_develop_source"]` has `.shape = "lua-values"` and `.properties[AdobeName] = exact Lua literal string` (`lua_develop.rs:695-738`), or shape `xmp-fragments` with original input fragments (`xmp.rs:209-223`). Lua booleans/numbers/nil are lexical JSON strings, with surrounding value whitespace trimmed. They are not typed JSON values generated through synthetic XMP. Explicit retention families include Upright*, ExtendedToneCurve*, masks, LensBlur, RetouchAreas/Info and PointColors (`lua_develop.rs:759-770`). The blanket original diagnostic/retention description must therefore be read with these qualifications.

## Dependency versus remaining gates

The retained-contract report committed in `8d61adce` reviewed integrated `0c6ab1c7`. I found **no diff from `0c6ab1c7` to `2164d370`** in import-lrcat, the shared CrsKey table or sidecar develop codec, so its contract findings remain current. B5-29c/29d provide tagged raw-text/cell-descriptor cases and lossless external-cell publication/resume (`import-lrcat/src/lib.rs:223-299,715,1191-1241`; `tessera-ffi/src/lrcat.rs:1154-1186`). Preservation/transport dependencies are satisfied; decoding or retranslation of external cells is not automatically supplied by that transport.

Before LR-1+ implementation: obtain the required coordinator matrix/ownership approval, approve needed representations and unsupported/cloud-only outcomes, and establish Adobe grammar/type/math evidence plus synthetic independent oracles. Retention does not resolve Point Color, B&W eight-band/PV legacy conversions, HDR curves, retouch consumers, mask assets or stored Upright semantics. When an LR-2 key is promoted into CrsKey, explicitly preserve its intended raw provenance: its current retention eligibility comes from being unmapped and otherwise disappears (`retain_source:770`). Consumers must handle all tagged shapes, including missing/omitted external data, without assuming `.properties` exists.

**Discrepancies resolved:** original 199/143/56 → current 201/143/58; two previously unnamed Upright counts now explicit; the fifth extended-curve Name key now explicit; lexical/tagged retention and grouped identity-aware diagnostics supersede earlier broad statements. No additional mapping-set discrepancy was found. Source review is not coordinator approval, executed RED/GREEN, or Adobe fidelity evidence.
