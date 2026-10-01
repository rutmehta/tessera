# LR-4 parametric mask extension

This is partial Adobe-format coverage, not a claim of Adobe pixel parity.
All committed fixtures are invented. No catalog or original image was used.

## Recipe and render contract

`MaskComponent` gains two additive fields. Existing components serialize exactly
as before; no format/version number changes and no new `MaskKind` variant.

| Field | Absent behavior | Present behavior |
| --- | --- | --- |
| `enabled` | true; true is omitted when serializing | false skips the entire component/subtree, including validation of inactive leaf geometry and external-raster requests |
| `group` | use the existing flattened `kind` | ordered child components replace the fallback kind; `Some([])` is distinct from absence |

A nested wrapper's `invert` applies after child composition; its `combine`
applies to that resulting plane in the parent. The first enabled component seeds
the accumulator, even if its combine is subtract/intersect. Subsequent add uses
max, subtract uses `a * (1-b)`, intersect uses `a*b`, preserving existing engine
semantics. An all-disabled top-level list stays empty despite group inversion.
An enabled empty nested wrapper produces zero before its own inversion.
Guided refinement runs once on the outer composite, before outer inversion.

Imported group wrappers carry an empty-brush fallback. Old JSON readers can
ignore `group`/`enabled` without an unknown enum error, but they **cannot render
these extensions faithfully**; inverted wrappers are especially not an empty
selection in old readers. Native sidecar output carries `ts:GroupFallback` so
arbitrary fallback kinds, including empty nested arrays, round-trip exactly.

The CPU and external-raster compositors understand recursive groups. Active-leaf
traversal feeds RGB-dependent cache keys, AI export/requests and FFI window
geometry mapping. Structural validation bounds a local group to 64 nesting
levels and 65,536 components before raster allocation. GPU resident mask metadata
skips disabled components and explicitly rejects nested groups, allowing callers
to choose their existing CPU path. This lane does not add a recursive GPU shader
or a native UI for editing individual nested children.

LR-5 can add a raster leaf kind under any group without changing group semantics.
Its integration must still classify/request that leaf through `MaskKind` and the
external-raster callback; this lane does not implement `Mask/Image` resources.

## Decoder coverage and limitations

- Existing gradient/radial geometry and local parameter mapping are unchanged.
- `MaskActive` is represented for every decodable component.
- `Masks` under `Mask/Group` or `Mask/Aggregate` preserves ordered subtrees.
- `CorrectionRangeMask` can constrain the union of a correction's masks or one
  nested component. Its `Invert` combines with component inversion using XOR.
- Explicit scalar `LumMin`/`LumMax` and `DepthMin`/`DepthMax` decode to the existing
  range kinds; `Mask/RangeMask` and `Mask/Range` are accepted spellings. Exactly
  one complete bound family is required. Bounds must be finite, ordered and in
  0..1. A caller must supply a same-level normalized depth plane for CPU rendering.
- Nonzero foreign range feather uses the existing scalar representation but is
  source-retained: Adobe's falloff/units have not been established by render
  comparison. Only zero-feather scalar fixtures are source-promoted.
- Opaque Adobe `Dabs`, `AreaModels`, `PointModels`, `LumRange`, subtype-coded
  ranges, and radial `Flipped` remain unresolved. Existing native brush/OkLab
  sample RDF support is preserved; it is not an Adobe brush/color decoder.
- Four-bound `LumRange` has asymmetric falloffs not represented by the current
  single-smoothness range operator. Color model payloads lack a verified sample
  color-space/encoding. Brush command and pressure/spacing semantics remain
  unverified. Guessing these would not meet the requested fidelity standard.
- `Mask/Image` stays source-retained atomically for LR-5. Depth-resource lookup,
  `DepthBasedCorrections` corrected-depth edits and `RangeMaskMapInfo` are not
  implemented here. Local curves/PointColors/grain are not added to LocalParams.
- Existing numeric blend-code and local-slider mappings are preserved. Their
  equivalence to every Adobe process/version remains unverified. Tests prove
  engine composition and local exposure on synthetic pixels, not an Adobe render.

## Source retention and B5-29c

The live retained envelope remains
`unknown.lrcat_develop_source.properties.MaskGroupBasedCorrections`, tagged
`lua-values` or `xmp-fragments`. Raw XMP packets remain separately preserved.
Only a unique, successfully decoded parent with a newly supported shape and a
complete structural audit can lose its per-key envelope. The audit rejects
unknown, misplaced and duplicate fields. Nil/undecodable rows retain their exact
source. Unrenderable defringe/color-overlay parameters, invalid adjustment amounts and
active geometry outside the CPU bounds do not get promoted. Duplicate Lua keys
are rejected by the pre-existing parser before a recipe exists; duplicate XMP
fields remain source-retained.

Legacy flat groups deliberately keep the old envelope and diagnostics. Default
component serialization is byte-identical. A 44-group invented fixture was
serialized on the untouched starting commit `87ff1ff1` and pinned without
repinning: 81,809 bytes, domain-separated Digest
`aec3a2eb9f1a31a1596d063b27ba785ae1d55219ba1931745c79a7cf9d8043cb`.
The pre-existing 2,000-row retained-source golden is also part of the gate.
These are synthetic compatibility checks, not a scan of any real catalog.

Decoder edits in `lua_develop.rs` and `xmp.rs` are small additive conditions;
translation remains in `sidecar`, and the source-consumption audit is isolated in
`import-lrcat/src/mask_source.rs`. No dependency or Cargo.lock changes are needed.
See the lane HANDOFF for exact commits, observed gate results and pixel errors.

## Source evidence inspected

The [ExifTool XMP tag inventory](https://exiftool.org/TagNames/XMP.html) identifies
mask/range fields without specifying opaque payload decoding. An
[Adobe patent example](https://patents.google.com/patent/US20240078730A1/en)
includes scalar range fields but is not a normative rendering specification.
A [published LRTimelapse XMP example](https://forum.lrtimelapse.com/Thread-issue-with-luminance-range-mask-intersected-with-linear-gradient-mask?pid=57463)
shows `Mask/RangeMask`, a subtype code and four-bound `LumRange`; only the schema
was inspected, and no example rows were copied. Conflicting third-party blend,
slider and `Flipped` descriptions were not treated as proof of Adobe parity.
