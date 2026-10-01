# LR-4c: review corrections and auditable approximations

This supersedes the LR-4/LR-4b claims about source promotion, schema version,
range luminance and GPU fallback. All new fixtures are invented; no real catalog
or RAW fixture is used by this lane's gate.

## Approximation contract

A successfully audited foreign mask maps into renderable recipe fields, retains
its exact Lua value or XMP property fragment in
`lrcat_develop_source.properties.MaskGroupBasedCorrections`, and records
`lrcat_develop_diagnostics` entries with `key`, `level: "info"` and
`message: "approximate: <reason>"`. These entries never enter the warnings vector
or the user-facing unsupported count. The matrix guard checks the populated
recipe path, exact source, field-specific info reason, and zero warnings. Its
negative controls independently remove fields/source/info, alter source, and
introduce a warning.

No LR-4 mapping claims verified Adobe convention or pixel equivalence. Unknown
or malformed grammar remains unsupported/source-retained. The legacy flat
shape path preserves the pinned 44-group and 2,000-row envelopes exactly;
new audited shapes (including documented metadata) use the approximation contract.
A neutral `MaskValue=1` alone does not opt an existing flat gradient into the
new diagnostic envelope; it is accepted when other fields trigger the audit.
Non-neutral MaskValue and all dab forms still trigger the audit.

### Explicit assumptions

- `Dabs`: `d x y` is an individual normalized-coordinate stamp; `r`, `f`, `h`
  update radius, flow and hardness for following stamps. Stamps never acquire
  connecting native brush segments. Radius is relative to image width, flow
  maps from 0..1 to 0..100, and feather is `(1-hardness)*100`. Tessera's smoothstep
  falloff and linear opacity are used. Nonzero `MaskValue` scales stamp opacity;
  zero-value Paint subtracts its stamp coverage from prior components. It is not
  an erase stroke evaluated on an empty plane.
- `PointModels` / `AreaModels`: the leading triple is assumed display-encoded
  sRGB D65, decoded and converted to OkLab. Remaining sample-position, area and
  reserved values remain in exact source. Area selection is approximated by its
  sample color, not by an invented spatial/ellipsoid kernel. Unit `ColorAmount`
  maps to OkLab distance tolerance with Tessera's smoothstep. Type 1 chooses this
  interpretation; malformed tuples, mixed range families and unknown types fail.
- Luminance: both scalar bounds and four-bound `LumRange` operate on sRGB-display-
  encoded Rec.2020 luminance (`Y = .2627 R + .6780 G + .0593 B`). The transfer is
  `12.92Y` below .0031308 and `1.055 Y^(1/2.4)-.055` otherwise. Negative/HDR values
  are not clipped into the selection range. This is a documented perceptual
  approximation, not a verified Adobe transfer. Four-bound shoulders use
  `[outer_low, low, high, outer_high]` with independent smoothstep transitions.
  The resident GPU scalar-band shader and FFI luminance eyedropper use the same
  transfer, so a picked pixel remains selected by the resulting range.
- Depth: thresholds apply to a supplied or Tessera-estimated normalized depth
  plane, not Adobe's depth calibration. Missing depth remains a runtime input
  requirement; resource recovery is LR-5/LR-6.
- Radials retain the existing normalized-coordinate rotation. On nonsquare
  frames that is not a verified pixel-space Adobe ellipse. `Flipped` is interpreted
  as the complement of `MaskInverted`; conflicting flags are refused. Midpoint
  and roundness are retained with an info note; the renderer uses its elliptical
  smoothstep shape. The lane does not claim to implement those Adobe modifiers.
- Blend codes 0/1/2 remain add/subtract/intersect assumptions. IDs, sync IDs, names
  and versions no longer block the audit. Zero-valued local toning fields are
  neutral; unsupported nonneutral overlay/defringe still prevent promotion.

The grammar references are the public
[Autoshade dab reader](https://github.com/skymanbp/autoshade/blob/main/src/xmp/read_corrections.rs)
and [range writer](https://github.com/skymanbp/autoshade/blob/main/src/xmp/mask_xml.rs),
plus [ExifTool's XMP field inventory](https://exiftool.org/TagNames/XMP.html).
They do not establish Adobe-rendered chart parity. No public source values were
copied into the synthetic fixtures.

## Rendering and consumers

GPU Camera Raw admission declines enabled nested groups, AI/depth leaves and
four-bound luminance leaves before tone/color dispatch. CPU parsing still reports
missing AI/depth inputs explicitly. A real GPU compositor regression compares
nested-mask fallback against CPU pixels. Smart-preview validation and MCP mask
coverage inspect active leaves recursively, respecting disabled subtrees.

Component-level range constraints apply `(not seed) intersect range`, preserving
the range's own inversion. They never invert the combined seed/range wrapper.

All mask coordinates address the pre-geometry active image in sensor orientation.
Local masks execute before Upright, manual transform, crop and output orientation.
The guided-Upright regression proves that rendering locals then geometry matches
the complete pipeline, and that evaluating the same mask after Upright differs.
LR-7 must preserve this ordering or explicitly transform mask coordinates.

FFI brush painting skips disabled brush components. Group metadata exposes a
`Group` component type, full tree JSON, and an `enabled` flag; disabled parts are
not marked rendered or queued for AI. Nested sanitization drops only invalid
children, matching top-level behavior. Swift binding regeneration/integration is
for the coordinator; no Swift gate or app launch is part of this lane.

Tree validation allows at most eight component levels (root components count as
one), including disabled branches, and 65,536 nodes. The importer audit and codec
also bound recursion. CPU rasterization adopts the first child buffer as its
accumulator and recycles temporary planes across siblings. External-raster
composition also adopts owned first-child buffers rather than copying through
Arc at each level; cached AI rasters remain immutable. Cache identity continues
to hash the full serialized tree.

## Conditional schema 4

Main `02ae8196` supplied the schema harness with an empty predicate list. LR-4c
registers exactly three features: `mask_component_disabled`, `mask_groups`,
`mask_luminance_bounds`. Presence is checked recursively, including disabled
subtrees, retouch areas and typed history-base settings. No additional recipe
fields are introduced. `RECIPE_SCHEMA_VERSION` remains 3; absent/default mask
fields retain their old bytes. Recipes using these fields serialize as 4 without
mutating the caller, and a loaded 4 remains sticky. Each feature has the shared
`assert_bumped_only_when_present` test; a journal regression verifies the FFI
save envelope stores 4.

Importer version checks distinguish recipes with/without v4 features. Sidecar
and merged-DNG equality tests separately check the written schema and normalize
only that field before comparing the in-memory recipe. The synthetic catalog
fixtures use no new feature, so their golden is unchanged. Four formerly opaque
LR-4b brush/color fixtures intentionally acquire fields and info diagnostics;
only those four pins are updated. Unsupported fixture pins remain unchanged.
