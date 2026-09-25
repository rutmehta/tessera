# M2-08 masks and local adjustments

## Engine behavior

`pipeline_cpu::masks::rasterize` accepts scene-linear Rec.2020 RGB at the desired
pyramid level. Pixel centres use normalized coordinates over that level's extent.
It supports linear gradients, rotated elliptical radial gradients, pressure/flow
brush paths (quarter-radius stamps, erase, smooth feather), luminance and OkLab
colour ranges, and normalized supplied depth planes. Composition is ordered:
max for Add, a*(1-b) for Subtract, a*b for Intersect. The first component ignores
its combine mode. Component inversion precedes composition; group inversion
follows optional guided refinement. Empty groups select nothing, even inverted.
See masks.rs module documentation for exact feather and brush semantics/bounds.

`locals_image`, `adjust_local`, and `blend_local` expose the reference application
path. Both the synchronous renderer and image-core execute locals after Color
and before Effects/Geometry. All groups sample the immutable pre-local image,
including range masks, and accumulate masked deltas per spec 01 §2.0. This is
order independent up to floating-point summation. The generic LocalsSettings
comment says list order; this implementation follows the more specific spec's
additive-delta rule (there are no local curves in the contract).

Amount is 0..200 percent and scales parameters before evaluating the operators,
as specified by LocalAdjustment.amount, not the final alpha. Thus +1 EV at 200%
is +2 EV, not a 2x alpha blend. Disabled, zero-amount and empty groups are skipped.

Local tone, texture/clarity/dehaze, saturation and positive sharpening/noise reuse
the existing global operators, same linear Rec.2020 working space and OkLab
colour space. Detail defaults are explicitly disabled before setting local
sharpness/noise so neutral masks do not add native revision-2 sharpening/NR.
Temperature is a relative CAT16 adaptation from the 6504 K Planckian white to
6504*2^(-temperature/100) K, tint uses the existing Duv scale with reversed target
sign. Hue rotates signed OkLab a/b after amount scaling. Negative sharpness
reduces detail via the global luminance NR operator. Negative noise adds back
its removed residual, without random synthesis. Moiré is a validated no-op
placeholder, as requested. These are native operators, not Adobe pixel matching.

## Cache and GPU

`image_core::MaskRasterCache` is an f32 byte-budgeted LRU. Keys cover procedural
components/inversion, pyramid level, dimensions, upstream Color hash, depth and
runtime options. RGB-dependent masks/refinement also include exact RGB content.
Local parameters, amount, UI name/id and enable flag are excluded. Pure geometry
therefore reuses alpha across cold-f32/warm-f16 upstream cache transitions.
RGB-dependent masks conservatively recompute if that transition changes pixels,
then reuse on stable inputs. Each renderer has a separate mask budget equal to
its tile budget; both budgets count retained payloads, not caller-held Arcs.

GPU alpha blending is an actual WGSL kernel, tested on Metal with <=1e-4 absolute
linear error and transfer/submission assertions. Oversized dispatches fall back
to CPU. Masks and local adjustment evaluation remain CPU reference work. Local
recipes use nonresident tile delivery and cannot silently bypass Locals via a
resident surface. Their upstream nonresident operators compute in f32 to avoid
amplifying resident f16 checkpoints through local exposure. Recipes without
locals retain their existing path and golden fixtures are not changed.

## Contract gaps (engine-api unchanged)

- ColorRange has tolerance (`amount`) but no smoothness. `MaskOptions` supplies
  runtime color_smoothness, default 50.
- No general guided-refinement radius/epsilon or mask feather/edge controls.
  `MaskOptions.refinement` enables the guided filter with level-pixel radius.
- Depth has a band/model reference but no plane binding. `MaskOptions.depth`
  accepts a same-level normalized plane. The RAW graph has no depth provider;
  a depth mask there returns an error instead of fabricating depth. Direct CPU
  raster/local APIs and the image-core raster cache accept supplied planes.
- Brush density/auto-mask and local curves, Point Color, and grain are absent.
- Existing defringe/color_overlay are outside this WP's requested slider set and
  return explicit errors when non-neutral. AI segmentation also returns an
  explicit unsupported error rather than pretending to have a model output.

## Verification

The requested three-crate release suite includes analytic masks at multiple
levels, radial rotation/feather/inversion, brush interpolation/pressure/flow/erase,
range selection, guided filtering, invalid inputs and resource bounds, all local
slider effects, exact masked +1 EV, amount/hue invariants, cache invalidation and
budget tests, graph ordering, GPU blend/chain tolerance and unchanged RAW goldens.
The full command and results are recorded in verification.log alongside this file.
