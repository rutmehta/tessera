# Dependency licensing

Checked the resolved versions' Cargo package manifests and the native macOS
`cargo tree -p typography` dependency closure. All runtime/build dependencies
in that closure offer permissive licensing. No GPL/LGPL/AGPL dependency added.
The bundled test fonts use OFL-1.1, with their full licences alongside them.

| Direct dependency | Resolved | Licence |
|---|---|---|
| fontdb | 0.23.0 | MIT |
| rustybuzz | 0.20.1 | MIT |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 |
| unicode-linebreak | 0.1.5 | Apache-2.0 |
| lyon_path | 1.0.19 | MIT OR Apache-2.0 |
| tiny-skia | 0.11.4 | BSD-3-Clause |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| psd (workspace) | 0.1.0 | MIT OR Apache-2.0 |

Transitive packages checked: adler2, arrayref, arrayvec, autocfg, bitflags,
bytemuck, cfg-if, core_maths, crc32fast, euclid, flate2, itoa, libc, libm, log,
lyon_geom, memchr, memmap2, miniz_oxide, num-traits, proc-macro2, quote,
serde_core, serde_derive, simd-adler32, slotmap, smallvec, strict-num, syn,
thiserror-impl, tiny-skia-path, tinyvec, unicode-bidi-mirroring, unicode-ccc,
unicode-ident, unicode-properties, unicode-script, version_check, zmij.

Their licences are MIT/Apache-2.0 alternatives, BSD-2/3-Clause, Zlib, or
0BSD/Unlicense alternatives. `unicode-ident` additionally requires Unicode-3.0
(the permissive Unicode data/software licence). Retain upstream notices when
redistributing compiled applications. tiny-skia uses its BSD-licensed Skia
port, not a system font rasterizer or a copyleft FreeType configuration.

The test binaries' copyright and OFL notices are in `tests/fonts/`.
