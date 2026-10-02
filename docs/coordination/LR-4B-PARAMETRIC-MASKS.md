# LR-4b: explicit ranges and radial inversion

Parent is `a88440a4`; this work does not rebase or rewrite LR-4. This is a partial
completion of LR-4b, not full Adobe mask fidelity. Brush and color remain blocked.

## Additive contract

`MaskComponent.luminance_bounds: Option<[f32; 4]>` is serde-defaulted and omitted
when absent. No format/version bump, enum variant, dependency or lockfile change.
Older readers ignore the optional field and retain the two-bound fallback;
they cannot reproduce the new shoulders.
The order is `[low_feather, low, high, high_feather]`. It is valid on an enabled
luminance leaf only; values must be finite, ordered and within 0..1. An absent
field keeps the old two-bound/smoothness operator and serialized bytes.

With the field present, CPU selection is one inside the inclusive `[low, high]`
band, zero outside the outer bounds and cubic smoothstep across each independent
shoulder. Collapsed shoulders are hard steps without division by zero. This
preserves asymmetric support and permits shoulder widths above 0.5. It uses
Tessera's existing scene-linear Rec.2020 luminance; synthetic tests establish this
operator, not equivalence to Adobe's internal luminance domain or transition
curve. FFI sanitization and the external-raster compositor preserve the field; native XMP carries it as a typed
`ts:luminance_bounds` field, and mask validation runs before CPU allocation.

The GPU metadata builder explicitly rejects the field with a CPU-path error.
Image-core already uses the nonresident CPU local-adjustment path; the Camera
Raw resident chain also propagates the unsupported-operation error to its
existing CPU fallback. No new shader silently treats the band as two-bound.

Foreign `LumRange` strings require exactly four numbers. Mixed scalar/four-bound
or color/depth geometry is refused atomically. `Type=2` accepts supported
luminance geometry; `Type=3` accepts scalar depth geometry. A code never invents
missing geometry. Type 1 stays opaque with its color payload. Nonzero legacy
scalar feathers retain the prior source-promotion restriction.

`Flipped` is treated as the complement of `MaskInverted`, not an independent XOR
bit. Both present must agree; otherwise the prior atomic retention behavior is
preserved. The net bit applies once after a component's range composition.
When `Flipped` is absent, existing radial decoding stays unchanged.

Catalog import enables these foreign extensions only after the existing complete
parent structural audit. If that audit or the decoded geometry/renderability
check fails, sidecar uses its prior foreign-mask decoding behavior. This keeps
untranslated recipes unchanged as well as preserving their exact raw envelopes.
Native typed fields continue to decode regardless of that foreign upgrade gate.

## Why brush/color are still blocked

The checked-in brief, rulings and prior handoff contain no normative dab-command,
color-model or rendering specification. The public sources below establish
parts of the grammar, but do not justify claiming full Adobe fidelity:

- [ExifTool's own XMP inventory](https://exiftool.org/TagNames/XMP.html) identifies
  the fields and types, not their rendering algorithms.
- [Autoshade's brush implementation](https://github.com/skymanbp/autoshade/blob/main/src/render/brush.rs)
  documents controlled measurements of Adobe flow and hardness. Its measured
  response differs from Tessera's linear-flow/smoothstep brush. It is an
  empirical approximation with stated residuals, not an exact Adobe operator.
  Mapping `Flow*100` and `(1-CenterWeight)*100` into Tessera and removing source
  would therefore misrepresent fidelity. Native paths also interpolate points,
  whereas a decoded dab sequence needs stamp semantics and erase composition.
- [Autoshade's XMP reader](https://github.com/skymanbp/autoshade/blob/main/src/xmp/read_corrections.rs)
  recognizes the `d x y`, `r radius`, `f flow`, `h hardness` token grammar.
- [Autoshade's XMP writer](https://github.com/skymanbp/autoshade/blob/main/src/xmp/mask_xml.rs)
  gives evidence for type-coded ranges and complementary radial flags. Its color
  writer still identifies sample-position/reserved entries as assumptions;
  it does not establish an OkLab interpretation of Adobe RGB-looking samples.
- The [JarvisArt paper](https://papers.neurips.cc/paper_files/paper/2025/file/4ac4365b98bc242acd5ab974a05c68a8-Paper-Conference.pdf)
  shows the six-value `PointModels` grammar in a Lightroom-produced workflow.
  It supplies neither a sample color-space specification nor the area-model
  selection and amount kernel.

No public source values were copied into fixtures. All coordinates, strings,
pixels, sample colors and IDs in tests are invented. No real catalog or image
was opened. No best-effort brush/color translation is advertised as complete.
The missing prerequisite is a verified encoding/render contract or authorized
controlled Adobe output measurements, especially for area models and color
amount. LR-4b's request for full fidelity does not authorize substituting an
unverified approximation.

## Evidence

Tests cover Lua and XMP decoding/source promotion, source retention, additive JSON
and native XMP round trips, invalid/mixed bounds, CPU exposure pixels and explicit
GPU refusal. See the handoff for exact counts, hashes and measured channel error.
The unchanged 44-group golden and baseline-pinned unsupported cases protect
compatibility. All gates exclude RAW-fixture tests and use synthetic inputs.
