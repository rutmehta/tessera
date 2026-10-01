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
