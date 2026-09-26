# M5-26 round 2 result

RESULT: PASS (required gate; implementation and limitations below)

This report supersedes the earlier blocker and intermediate worker notes. Work remains uncommitted on `wp/M5-26`. The pre-existing brief change was retained. No Kanban task ID was present in this session, so no board state was changed.

## Delivered

- CPU evaluation and resident GPU ports: Brightness/Contrast (legacy/modern), Vibrance with saturation/skin protection, Color Balance, Black & White with tint, Photo Filter with presets/custom color, Gradient Map (Classic/Perceptual/Linear, reverse, real spatial dither), Selective Color (relative/absolute, nine groups), Desaturate, Equalize, Auto Tone/Contrast/Color, Match Color, Replace Color and Color Lookup.
- Equalize/Auto store resolved histogram parameters. Match Color stores source layer identity and frozen Lab statistics, with a source-layer resolving constructor and an explicit raw-sample constructor.
- Version-1 serde envelope and validation, without changing the existing tagged enum document representation. Invalid LUT structures return renderer errors rather than panic. Named Photo Filter presets are documented native approximate sRGB swatches.
- Checked unit-domain 3D CUBE and uniform-grid 3DL loaders; real LCMS abstract and RGB device-link ICC sampling. Red-fastest cubes use identical CPU/GPU trilinear evaluation.
- PSD mappings and serialized roundtrips: brit/CgEd, vibA, blnc, blwh, phfl v2 RGB, grdm v1/v3, selc, clrL embedded CUBE. SoCo remains solid fill. No native JSON disguised as Adobe keys.
- Positive-radius Shadows/Highlights live CPU fallback: `Compositor::render_tile_with_neighbourhood`. Replays padded backdrop prefixes across tile boundaries with a fresh zero-cache compositor, handling groups, masks, alpha, opacity and modes. Tests cover L0/L2, neighboring edits and sequential adjustments. Identity/zero-radius Shadows/Highlights also has a bit-exact resident port.
- COMPOSITOR.md §4 documents controls, formulas, serialization, interchange and limitations. §12.3 records new parity coverage.
- Self-review fixes: spatial rather than color-hash dither, accurate Replace Color RGB-distance documentation, structural validation at both compiler entrances, and aggregate resident auxiliary allocation/binding limit checks.

## Verification executed by the parent

With `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26`:

`cargo test -p compositor -p psd --release && cargo clippy -p compositor -p psd --all-targets -- -D warnings && cargo fmt --check`

Exit 0. Release suite: 262 passed, 0 failed, 10 ignored across 47 reported suites. Includes 7 M5-26 GPU tests (real GPU, to_bits equality, L0/L2, F32/U8/U16, interpreter/specialization), 11 keyed PSD tests, 14 local/fallback tests, 4 ICC integration tests and direct-deserialization validation. Full output: `gate.log`. Existing LibRaw C/C++ compiler warnings are emitted but do not fail the Rust lint gate.

Also executed `cargo test -p color-mgmt --release`, exit 0 (`color-mgmt-tests.log`). `git diff --check` passed. Programmatic allowlist audit found no out-of-scope changed or untracked paths in this worktree. No build output was placed in the worktree.

## Not done / restrictions

- HDR Toning is not implemented.
- Positive-radius Shadows/Highlights is not GPU accelerated or automatically uploaded through resident rendering. Ordinary cached CPU/resident paths return Unsupported and name the explicit live CPU fallback. Its conservative prefix replay can be expensive. Layer styles and neighborhood adjustments inside smart-object child documents are unsupported by this fallback. Black/white clip controls are normalized endpoints, not Photoshop histogram percentiles.
- No claim of Photoshop proprietary numerical identity. Tests establish native formula expectations and CPU/GPU identity on the precise Metal path, not Photoshop application output. Perceptual operators assume sRGB; arbitrary document-profile conversion is not automatic. Other GPU backends retain the existing relaxed-precision caveat.
- Match Color's layer constructor analyzes raw pixel/text-proxy content, excluding transparent samples, not masks/effects/opacity or rendered groups. It rejects tagged documents and non-raster sources. Source edits require resolving the one-shot again.
- `.look`, 1D/shaper CUBE, non-unit CUBE domains and nonuniform 3DL grids are unsupported. ICC loader accepts abstract and RGB→RGB device-link profiles, not arbitrary profile classes/color spaces. Large valid LUTs may exceed a device's checked resident limits.
- PSD Photo Filter v3/non-RGB, unsupported gradient features, and clrL representations other than embedded unit-domain 3D CUBE remain opaque. Desaturate, Equalize, Auto, Match Color, Replace Color and Shadows/Highlights are native-only layer representations with explicit PSD-export errors.
- Pre-existing per-range Hue/Saturation remains outside this implementation.

## Integration hotspots

`resident/mod.rs` has shader inclusion and auxiliary-size preflight wiring only. `render/exec.rs` contains additive fallback/prefix execution and the spatial-dither call site. These are the overlapping files named by the package brief; reconcile those small entry-point changes carefully with sibling work.
