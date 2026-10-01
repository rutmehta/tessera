# LR-2b — Machine B handoff

Branch: `wp/LR-2-tone-curves`. Successor of LR-2 at `26e5cb8a`, with no rebase.
Local commits only. This handoff supersedes the LR-2 status at that base.
Final gates are recorded in EVIDENCE.md; no Adobe pixel-parity claim is made.

## Commits

- Base: `26e5cb8a`.
- Initial RED: `aaa68ad459096ffa0d87e104b4b5408a6a4cb998`.
- Develop admission RED: `03d0f5b94824a9bd10cb7ff9a44f2ed205ef2694`.
- Implementation/GREEN: `bb4b3e6e347cdc28761488c11b14c33b3a798ac0`.
- HDR follow-up expectations: `0de9fc9b` (test-only correction after full gates).
- Documentation follows in a separate `docs(LR-2b):` commit.
- All LR-2b commits end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

The supplied, untracked `LR-RULINGS-FROM-A.md` is not edited or staged.
No Cargo.lock, Cargo.toml/dependency, board, mailbox, Swift, app or real-catalog work.

## Ruling status

### Ruling 2 — approximate-with-reason; dedicated branch and guard implemented

Added optional `/settings/tone/legacy_pv2010`, with optional original-value members
`exposure`, `brightness`, `contrast`, `fill_light`, `recovery`, `blacks`. Serde
defaults, omission when absent, and no schema/process/FORMAT bump. Missing fields
have no effect; no camera defaults are invented. Imports require **Adobe** PV1/2,
not merely revision <=2 (native revision 2 is explicitly tested).

The old PV2010-to-PV2012 slider conversion is removed. Exposure2012 still suppresses
legacy Exposure and Brightness; other modern tone keys suppress their legacy
counterpart. HighlightRecovery precedes Recovery; Shadows precedes Blacks.
Modern process images retain stale legacy source unchanged.

The CPU branch performs the operators independently. Develop now admits Adobe
PV1/2; both `pipeline-adobe` and `image-core::AdobeStageOp` use the branch. Native
GPU Tone rejects legacy settings before dispatch, including fused/resident
parameter construction. Adobe recipes use the existing CPU compatibility barrier.
There is no silent legacy GPU output and no claim of a native legacy GPU kernel.

Per-parameter exactness:

| Parameter | Status / why exact Adobe reproduction is unavailable |
| --- | --- |
| exposure | Documented EV scaling `2^E` is implemented directly. This is not a claim about the entire Adobe camera/profile pipeline. |
| brightness | Approximate: available Adobe documentation gives behavior, not the numerical midtone curve or slider-to-gain calibration. Uses a bounded rational midtone curve with black/white fixed. |
| contrast | Approximate: no public PV2010 pivot, transfer or slider-to-slope calibration was found. Uses a monotone midtone odds curve. |
| fill_light | Approximate: the spatial/adaptive shadow algorithm is not specified publicly. The reference is scalar shadow fill and does not reproduce Adobe's spatial adaptation. |
| recovery | Approximate: the tone stage has working RGB rather than Adobe camera-channel clipping/reconstruction data; the recovery algorithm is not specified publicly. A highlight shoulder reduces highlights but cannot invent missing detail. |
| blacks | Approximate: uses the public Adobe DNG baseline per-channel quadratic shadow toe, assuming ShadowScale/Stage3Gain one. Camera-specific scale is unavailable in this block, and the SDK does not establish full PV2010 parity. |

The public DNG renderer was inspected in addition to Adobe's operator docs.
Equations, order, ranges, primary-source links and the exact SDK assumptions are
in [`crates/pipeline-cpu/LEGACY_PV2010.md`](../../../../crates/pipeline-cpu/LEGACY_PV2010.md).
Exposure's represented source key leaves pending source; approximate controls
retain original literals/fragments as well as their numeric fields. Goldens are
independent scalar reference calculations, **not Adobe-rendered calibration**.

### Ruling 3 — done (HDR representation and rendering)

Added optional `/settings/tone/curves_extended`, with the same shape as `curves`.
Signed/HDR source knots are normalized by 255 without clamping and routed here.
Ordinary channels are copied before extended per-channel precedence is applied.
When present, the block replaces ordinary curves in native CPU/GPU and Adobe CPU
renderers, including parametric/channel/luminance members. Invalid/nonmonotone
source remains retained. Extended splines do not acquire artificial SDR endpoints.

CPU/GPU use the same signed extension of the native log curve domain; the Adobe
compatibility renderer retains its own signed power-domain approximation. HDR
input bypasses clipping in the fallback SDR profile curve; supplied DCP behavior
is unchanged. The fields and HDR values are honored; Adobe-exact curve calibration
is not claimed. JSON/history round-trip, native control-point goldens, signed
luminance, all-channel GPU parity and full Develop dispatch are tested.

### Ruling 1 follow-through — done

Removed the monochrome GPU guard and implemented the same eight-band luminance
mix as CPU in WGSL, before ordinary color processing. Standalone and fused GPU
paths match CPU within the existing absolute `1e-4` operator tolerance, including
mixed band amounts and subsequent color grading. The existing Adobe
profile-dependent B&W approximation remains explicitly reported.

Sidecar changes are additive: reads/writes `ConvertToGrayscale` and all eight
`GrayMixer*` CRS properties, retains unchanged source spelling, and round-trips
the optional monochrome setting. The export path using `XmpPacket` inherits this
support. Native Develop validation now admits the already-existing monochrome
field. New tone blocks round-trip through recipe JSON/history; this lane's new
sidecar CRS export work covers monochrome.

### Ruling 5 — done

AutoToneDigest* exact values remain in retained source/internal per-property
payloads, but no digest warning reaches `ImportPlan.report`. The host's
“Not fully supported” grouping consumes that report (`tessera-ffi/src/lrcat.rs`),
so no Swift edit is necessary. Synthetic SQLite import tests verify report
suppression. DepthMapInfo still belongs to LR-5/LR-6.

## Compatibility and review boundaries

- Five unrelated Lua/XMP recipes: **56,666 bytes identical** to `26e5cb8a` using
  both the baseline importer and baseline sidecar compiled against unchanged API
  semantics. New optional fields are omitted when absent.
- The 2,000-row golden changes from
  `ad814642116dda65cf0a49494eee8a5cf034c2d3151cb8c35c69975f8cfd661a` to
  `174e43107e23125fb0477cad9376a144a35c28a7966de355ebe87314a98c8fee`.
  A baseline audit reproduces the old hash: **1,200 already-translated monochrome
  rows change history only** (sidecar now sets monochrome in its initial edit),
  with identical settings/source; **800 rows remain byte-identical**. The audit
  asserts that no other member differs.
- `docs/coordination/LR-TRANSLATION-MATRIX.md` was absent from this branch at the
  base. A scoped LR-2b matrix is added for the coordinator to combine with LR-0.
- Likely merge overlaps: ToneSettings, CPU/GPU tone and color preparation,
  image-core Adobe admission/dispatch, import `lr2` and extended-curve note,
  sidecar develop codec. No dependency additions are needed.
- Initial RED included seven runtime failures. Its digest assertion first failed
  on exact source quote spelling; that literal was corrected to the preserved
  single-quoted form. Separate RED checks caught Develop's PV1/2 rejection,
  black-toe scaling, independent black-channel mapping, and native/adobe revision
  confusion before their fixes. See EVIDENCE.md for commands and outcomes.
- The old E2E test assumed pending source always existed. Exposure is now fully
  represented, so a fully translated row can omit that map entirely; assertions
  now accept map absence while still requiring each translated key to be absent.

## Gates

Run `bash tools/orchestrate/wp/LR-2/gates.sh` with the requested lane target,
three Cargo jobs and three Rayon threads. It runs all seven touched crates,
clippy `-D warnings`, fmt, synthetic SQLite E2E, and the baseline byte audit.
Eight external-RAW tests are explicitly excluded; existing ignored benchmarks
remain ignored. No real catalog, external RAW image, Swift gate or app is used.

Final test coverage: **702 passed, 0 unresolved failures, 19 ignored, 26 filtered
instances** (eight named external-RAW exclusions across targets). The full run
reported 699 passed / three stale assertions in `import-lrcat --test followups`;
all other targets passed. After a test-only correction for ruling 3, that entire
16-test target passed (replacing its earlier 13 passed / three failed result).
The implementation was unchanged; unaffected targets were not rerun.
All-target clippy `-D warnings`, workspace fmt, synthetic E2E and the baseline
byte-identity/golden audit passed.

---

# LR-2c — Machine B review follow-through (2026-10-01)

This appendix supersedes the LR-2/LR-2b behavior and gate claims above.
STEP 0 completed: fetched origin and rebased all eight prior lane commits, without
squashing, onto `02ae81960e104376bcfa6545d39515c3d1592454` (origin/main at fetch).
The sole conflict was the matrix add/add. The final matrix keeps all 107 main
keys and adds Recovery/Blacks aliases. No import-hook conflict occurred on this
main revision. No push, board, Cargo.lock, dependency, Swift, app, or real-catalog
operation was performed. The untracked supplied rulings file remains untouched.

## Review blockers

1. **Implemented:** dedicated `legacy_pv2010` operators from LR-2b retained;
   unverified mappings now keep exact source and per-key info-level
   `approximate: <reason>` diagnostics, with no user-facing approximation warning.
   `AutoToneDigest*` remains silently retained cache metadata.
2. **Fixed:** extended curves never mutate ordinary curves. Nonidentity extended
   source populates `curves_extended` only with HDREditMode=1; identity/inactive
   source stays retained without altering settings. Tests combine both curve keys.
3. **Fixed:** disabled B&W/zero mixer and identity extended curves are strict
   settings/history/cache no-ops. `tests/golden.rs` is restored verbatim from main,
   digest `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.
   No re-pin. The no-op/hash/history regression is a Cargo integration test.
   `point-color-compat.txt` is absent on this main/branch and has not been created
   or changed; coordinator should retain LR-1's file unchanged when merging.
4. **Fixed:** legacy Tone reaches `cpu_fallback` in both standalone and batched
   GPU dispatch. Legacy recipes decline resident/fused tone dispatch. Synthetic
   native and Adobe GPU sessions render legacy+B&W+channel curves successfully
   and agree with CPU within the existing 1e-4 operator tolerance; direct legacy
   single-stage/batch outputs are exactly equal. A pure-B&W resident regression
   also passes with/without local tone under the established 0.002 f16 bound.
   Resident chains split into supported fused runs around pre-curve B&W so the
   effects tail never falls through to an unsupported scalar dispatch.
   LR-2b's GPU monochrome kernel remains supported; no colour-stage rejection is
   reintroduced.
5. **Fixed:** matrix `approximate` guard checks recipe path, exact source literal,
   per-key info reason and zero warnings, including negative controls. Grayscale,
   all mixers, active extended curves and all legacy operators are approximate.
   Residual Clarity, CurveRefineSaturation, GrainSeed, Incremental*, SDR*,
   ToggleStyleAmount/Digest, AutoTone and AutoWhiteVersion have explicit
   retained/unsupported dispositions; no invented Adobe calibration claim.

## Major findings

- Shadows=5 is stored unchanged as legacy blacks=5; current blacks stays zero.
  The dedicated black operator uses the original control, not a -5 offset.
- Brightness remains the LR-2b rational midtone operator; no exposure heuristic.
- PV2010 source wins over stale PV2012 controls; the modern controls are neutral
  in that branch. Modern process rows do not activate legacy settings. Native
  revision 2 does not pass the Adobe-family gate.
- B&W conversion now precedes point/channel curves across standalone CPU,
  Adobe compatibility, host GPU and resident GPU render paths. Ordinary colour
  controls follow. Channel-curve toning survives; B&W/mixer changes invalidate
  tone-stage caches only when enabled. Unaffected hashes retain their old form.
  Adobe ordering/calibration is unverified: this is an explicit Tessera reference
  order and is covered by the info-only approximate status.
- LR-2 executes once for Lua source; generated XMP skips that pass. Changes are
  consolidated into the import's single replayable initial edit, with undo/redo
  tests. Unchanged imports retain their prior history bytes.
- The LR-1 point-colour implementation is not on this main. This branch already
  clears `ordinary.monochrome` before its colour-neutral check. Keep that clear
  alongside LR-1's point-colour-only check when integrating; preserve both Lua
  and README hooks from LR-1/LR-7.

## Schema ruling: coordinator integration hook

The shared LR-SCHEMA `required_schema_version` helper is **not** on the rebased
main. Per the explicitly authorized fallback, LR-2c supplies
`DevelopSettings::required_schema_version_lr2()` and tests: v4 for enabled
monochrome, `curves_extended` or `legacy_pv2010`; v3 when absent (also for disabled
monochrome). Register its maximum with all other lane predicates in the shared
helper at merge. This lane does not independently change global reader/writer
policy. Do not ship/persist new-field recipes until the shared v4 writer and
older-build-refusal policy is integrated. No claim is made that this branch
alone already writes v4.

## Tests-first and validation

RED commit: `85662ed3`. Runtime RED reproduced inactive settings/hash/history
changes, curve overwrite, approximation warnings, wrong legacy precedence,
legacy GPU rejection, lost channel toning and unrecognized matrix status.
Additional RED cases exposed the B&W resident-chain/effects fallback and a
digest-only PV2010 row changing modern settings; both are covered and fixed. The
new schema-predicate test initially failed compilation because the API was absent.
Main's golden failed before the compatibility fixes and was never re-pinned.
Some superseded LR-2b assertions intentionally required warnings, source removal,
modern precedence, or GPU rejection; those now assert the binding LR-2c contract.
Two source-retention assertions also addressed the wrong JSON location
(`Recipe::unknown` serializes flattened); they now inspect the real retained map.

The reproducible gate is `tools/orchestrate/wp/LR-2/gates-c.sh`. It cleans touched
crates before testing the requested six crates plus the changed image-core and
pipeline-adobe crates. It lists 23 external-RAW exclusions explicitly; ignored
benchmarks remain ignored. Only synthetic media/catalog fixtures are used.
Detailed final outcomes and durable log locations follow in EVIDENCE-C.md.

Implementation commit: `c2df1958`. The broad gate also exposed a stale FFI test
expectation from before LR-2b expanded Adobe support to PV1–6. Its exact error
expectation now matches that supported range; revision 99 is still rejected, and
the no-writer-construction assertion remains intact. The focused rerun passed.
The Liquify latency assertion failed under load and in an isolated serial rerun;
no threshold or assertion was weakened. See EVIDENCE-C.md for measurements and
the final gate outcome.

Final six-crate gate: **1115 passed, 1 failed, 47 ignored, 37 filtered**.
The sole failure is the unchanged Liquify p95<250 ms timing check (266.9 ms
in the final run; 600.0 ms isolated serial). All lane regressions passed.
Eight-crate Clippy `-D warnings` and `cargo fmt --all --check` passed.
The gate is not fully green; see EVIDENCE-C.md for all runs and exclusions.
