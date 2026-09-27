# M5-30 implementation plan

The supplied brief is the accepted scope. All changes stay in its allowlist; no commits.

1. Add and observe failing regressions for vector-mask pixels, live layer/history/native models, modeled Adobe EngineData, and document wire calls.
2. Add serializable vector shape models, live compositor variants, validating atomic DocOps, and native persistence. Preserve history's copy-on-write semantics.
3. Rasterize shape paths and transformed glyph outlines at the requested output level. Cache source tiles by serialized model, transform, output coordinates, depth and dimensions. Use the same generated source and effective mask samples on CPU and resident paths, uploading generated resident pages directly rather than generating their mips from level zero.
4. Model Adobe text dictionaries and standard PSD vector/fill/stroke records, preserve unsupported payloads, write cached layer pixels, and test byte-built fixtures.
5. Extend engine-api 1.5 additively and expose document operations through MCP schemas and dispatch, with backward decoding and wiring tests.
6. Document host font ownership, coordinates, hit testing, edit/history units, raster cost and limitations in TEXT_VECTOR.md.
7. Review integration, run the full release test / clippy / fmt / workspace check gate with the mandated CARGO_TARGET_DIR, fix failures and report actual results.

Review focus: output-level geometry versus scaled proxies; UTF-8/run boundaries; unsupported PSD data surviving edits; mask feather across tile boundaries; native reopen and undo restoring exact live source; legacy API defaults.
