# Restricted RAW admission scaffold — source-only, UNRUN

Base342fcccc4b5e07e20893a5832b73ce41d8383ec9. Branch codex/raw-render-admission in the new managed raw-render-admission checkout. Implements only the reviewed plan's test/scaffold preparation step, not admission behavior or pixel rendering. No compiler, tests, formatter, app or GPU was run; FFI owns runtime. No expected-failure count is an observed result.

## Exact scope

`image-core/src/lib.rs` includes private `raw_admission` only under cfg(test). New private functions `admit_recipe`, `admit_metadata`, and `check_inventory` unconditionally return `Refusal::Unsupported`. All types and operations remain inaccessible outside the private module; there are no new dependencies, public exports, Engine API changes or production callers. Recipe and decoded inputs are borrowed. The scaffold cannot perform capture/decode, normalization, rendering, ICC I/O, allocation reservation or publication because no such operation or callback is present.

Eighteen synthetic unit contracts specify:

- Existing PinnedRawDescriptor authority is constructed first, including check_shape; no duplicate settings-schema parser. Omitted defaults and empty legacy camera reference remain compatible. Exact input recipe bytes remain unchanged. Supported exposure and recipe detail survive.
- Default Auto demosaic/lens, manual optics, explicit camera/lens references, neural/model dependencies, local adjustment, depth, LUT and HDR fields refuse, without settings sanitization.
- A descriptor unknown-key control exercises the already implemented authority and should pass on the scaffold; new behavioral contracts should fail Unsupported once compiled. Missing imports/fixture failures would be compile/setup defects, not meaningful RED.
- Owned synthetic CFA checks cover Bayer/layout/count/crop/orientation/correction metadata/calibration. Changed claimed captured digest/length does not alter descriptive metadata facts: this deliberately proves the function does NOT authenticate public DecodedCapturedCfa fields. The declared closed route is not classification and no classifier is introduced here.
- Explicit accounting inputs use tiny integers and no large buffers. Hand-derived case:24 sensor pixels*(2-byte U16+4-byte F32)+6 active pixels*8-byte RGBA+16 metadata+32 scratch=240 bytes, output48. Exact cap, each one-byte/pixel lower cap, zero/inconsistent extents and multiplication/addition overflow are specified.

The arithmetic function does not prove the scratch inventory complete, reserve bytes, cap previous native unpack, or bound the process. Numerical values are synthetic test inputs only. The metadata fixtures are invented values, not camera qualification or decoded provenance.

## Deliberately unavailable

No normalization-sharing change; the existing private normalization/clamp[0,1.2] is untouched. No renderer, actual memory cap/concurrency policy, allocation audit, ICC bytes/digest decision, environment bootstrap or persisted render-envelope implementation. No RAW node, FFI, Document adapter or public resolver. No trusted-environment dummy token that could be mistaken for implemented authority. Tests do not claim callbacks/terminal resource lifetimes for absent worker operations.

Future descriptor accessors are proposed, NOT added: read-only typed `asset_identity` parts (Digest, positive byte length) and `decoder_route` on PinnedRawDescriptor could allow a later private resolver to compare capture identity/route before decode. Engine API must not depend on raw-decode's CapturedAssetIdentity; return existing pure engine value types or separate getters. Accessor design requires independent approval. This scaffold needs only existing recipe_json(), avoiding wire JSON round-tripping. Eventual trusted resolver must itself own capture→closed decode→private metadata continuation; successful metadata facts never authenticate a caller-constructed decoded struct.

## Next gate, after independent source review and explicit runtime grant

Run `cargo test --jobs 2 -p image-core --release --lib raw_admission::tests -- --test-threads=1` under coordinator-selected shared target/deployment environment with before/after source/artifact manifests. Preserve warnings/errors, exact test inventory and direct exit. The intended first gate is behavioral RED from Unsupported, not GREEN and not missing fixtures. No formatter has run; mechanical formatting or lint corrections, if needed, must preserve this initial checkpoint and be disclosed.

After a valid RED and separate implementation authorization, implement only pure predicates/arithmetic, reusing descriptor compatibility and existing pipeline_cpu::validate_settings. Do not render as part of making these tests green. Broader source matrix coverage (all remaining unsupported settings/refusal precedence and calibration semantics) remains reviewable before GREEN. Real capture/normalization/ICC/environment/output tests stay blocked by the reviewed plan's prerequisites.


## Independent-review correction checkpoint

The initial c65b314f is retained. Source review identified missing coverage of the actual Native2 matrix: render.rs constructs/inverts first three cam_xyz rows, independently of stored camera_to_xyz. Added nonfinite values in every consumed cam_xyz cell, zero matrix and duplicate-row singularity controls; retained stored camera_to_xyz finite/invertibility checks as explicit conservative profile policy, not proof of consumed calibration. Black levels are checked in all four CFA channel slots against sensor white; white0 refuses. All first-three RGB white-balance multipliers receive zero/negative/nonfinite controls. Native2 RGB WB ignores multiplier4; new positive Bayer phase controls explicitly allow that unused slot0. The synthetic fourth cam_xyz row remains0 and is not required invertible.

Additional refusal coverage now includes every neutral LensSettings member (including inactive ranges), disabled local edits and retouch, Point Color, proof handle, non-default decode frame/pixel-shift, unsupported reconstruction/display transform and Auto white balance. The Auto-WB case requires an explicit predicate: general pipeline_cpu::validate_settings copies WB settings and does not itself reject Auto, while white_balance_matrix rejects it later. All16 Bayer phase/green-index combinations are positive controls (green1/1,1/3,3/1,3/3); these are synthetic syntax/shape acceptance, not camera qualification. No setting is rewritten on refusal.

All18 tests remain UNRUN; three operations still unconditionally Unsupported. There is no GREEN claim. No formatter/compiler ran. Full usable-color semantics beyond these calibration controls still need source review against the real downstream math before a renderer is contemplated; these pure tests do not establish actual pixels, native decoder memory bounds or proprietary camera calibration fidelity.

## Pure implementation source checkpoint (after observed b950941e RED)

A's exact Release compile exited0; focused18 ran with1 existing descriptor control passing and17 failures at Unsupported, not setup/compile. Immutable source/runner/binary evidence is `/Volumes/betterSSD/tessera-validation/raw-render-admission/b950941e`. This author now replaces only the three pure operations; the18-test source and assertions remain byte-identical to b950941e. No GREEN run yet. FFI owns runtime; compiler/tests/formatter were not invoked for this implementation checkpoint.

Recipe predicate parses only an already validated descriptor's retained recipe via Recipe::from_json, preserving descriptor bytes and existing omitted/legacy settings semantics. It checks the explicit narrow policy, Auto WB separately, then calls pipeline_cpu::validate_settings without sanitization. Metadata predicate uses checked size/count/crop checks, a complete2×2 Bayer with opposite R/B and either green code, no corrections/orientation transform, finite black/white/WB, conservative stored matrix checks and actual consumed cam_xyz inversion. It also checks usable as-shot scene-white using the existing scalar white_balance_matrix with default WB: invertibility alone is insufficient if the resulting white is invalid. This is matrix math only, not image rendering; the conservative unconditional as-shot check is explicitly reviewable, including when a future recipe requests custom WB. Fourth WB slot is unused, matching the Native2 RGB WB path.

Accounting computes the supplied partial inventory using checked6*S,8*A and additions before caps; this is no audited peak/reservation or native limit. Removed Unsupported enum case since implemented predicates now return concrete refusals. Module remains cfg(test), with no public or production resolver. Source review precedes any new runtime grant. Memory/normalization/ICC/environment and trusted capture orchestration stop lines remain unchanged.

## Explicit geometry defense and calibration regression

Reviewed `/tmp/tessera-raw-admission-implementation-review.md` for83e2698d. Added an explicit exact GeometrySettings::default guard to the profile. Important correction to the review's reachability inference: PinnedRawDescriptor::new already rejects non-default geometry (pinned_raw.rs129–134), from_json delegates to new, and retained recipe bytes are private. Therefore a valid descriptor cannot deliver the alleged non-neutral success at83e2698d. This is defense-in-depth and boundary coverage, not a demonstrated reachable behavior fix. No fabricated descriptor or unsafe bypass was added to manufacture RED.

Two appended tests bring the total to20: descriptor-boundary refusal for every crop/transform member, orientation/constrain/upright and inactive guides, preserving the caller recipe bytes; and invertible cam_xyz with nonpositive as-shot scene white, separate from singular-matrix refusal. Neutral descriptor and usable metadata remain positive controls. These added assertions are expected to pass even at83e2698d (the explicit redundant guard is not independently reachable); no new observed RED is claimed. Original18 test functions remain unchanged, new functions are appended. All20 remain UNRUN on the corrected implementation. No compiler, formatter, renderer, normalization or public API change occurred.
