# ISO gain-map JPEG restoration implementation plan

**Goal:** Export an SDR-compatible JPEG with an ISO 21496-1 version-0 gain map and MPF association, reconstructing the existing display-linear HDR rendition.

**Architecture:** Keep the existing CPU HDR render/orient/resize/sharpen pipeline. Derive an sRGB SDR primary plus a full-resolution grayscale logarithmic gain map, encoded with existing jpeg-encoder; add ISO APP2 metadata and MPF only. Use independent extraction/reconstruction and actual ImageIO SDR/HDR pixel readback for validation. No runtime dependency, Adobe XMP directory extension, native ImageIO encoder, GPU route, or changes to accepted metadata precedence.

**Spec:** `brief.md`, `HDR.md`, and original prototype/test in `codex-implementation.log` (approximately lines 10247–10408). Diagnostic evidence: `/tmp/tessera-gainmap-diagnostic-report.md`; pinned independent Skia control/provenance and exact programs in `/tmp/tessera-iso-gainmap-control/`.

## Constraints and evidence

- Start from accepted wp/M2-45d 69bcd3e in isolated codex/gainmap-restoration; stable export/integration checkouts remain untouched.
- Source preparation is authorized; heavy Rust/Swift builds wait for the coordinator's slot. No commit until validation passes and independent review completes.
- Preserve original patch criterion: `abs(actual-expected) < 0.04 * max(expected, 1)` for each RGB channel at five patch centers (x=8,24,40,56,72; y=8) in an 80x16 quality-100 fixture. Expected values come from independently inverse-transferred exported PQ PNG16 and Rec.2020-to-sRGB conversion.
- Current native decode vs independent libjpeg base/gain reconstruction has whole-frame worst normalized discrepancy 5.0658% at sharp boundaries (maximum absolute 0.12527). That is not a whole-frame 4% pass. Keep this diagnostic explicit; do not weaken patch assertions or silently add a looser full-frame threshold.
- Historical ImageIO nil-provider/aux failures are unreproduced on macOS 26.6.2 (25G83), arm64. Options and Foundation are not established causes. Independent Google Skia ISO regression fixtures and retained prototype now decode real opaque, finite SDR and HDR pixels. Apple fixtures remain separate Apple controls; no general interoperability/certification claim.

## Implementation and acceptance

1. [x] Restore `crates/export/tests/gain_map.rs` first. Confirm a lightweight compile-red against existing accepted export artifacts for missing `ExportSettings.gain_map`, before implementation; defer full execution until slot.
2. [ ] Add `crates/export/src/gain_map.rs` and minimal settings/render/encode routing in `lib.rs`/`hdr.rs`. Preserve cancellation, no-clobber publication, byte budgets including all metadata/auxiliary data, recipe gamut handling, privacy/native metadata, and CPU-only HDR behavior. Reject non-JPEG/non-sRGB, PQ/HLG combination, watermark, SDR enhancement hooks, disabled/zero headroom, malformed dimensions, and unsupported combinations before publication.
3. [ ] Independently parse MPF TIFF offsets/lengths and ISO rational metadata; decode both primary/gain images; reconstruct five patches against PQ reference with unchanged 4% criterion. ExifTool MPImage2 extraction must match independently parsed gain bytes.
4. [ ] Add native test-only ImageIO SDR/HDR decode that copies nonempty provider bytes then renders float RGBA in extended-linear sRGB, checks opaque finite pixels, and compares expected patch values. Negative controls remove ISO or MPF; ordinary SDR must remain decodable and HDR request must not invent missing gain. Maintain separate whole-frame discrepancy reporting.
5. [ ] Add bounded byte-budget/no-output failures, metadata/privacy carrier readback, cancellation/no-clobber checks, and headroom/gamut/resize coverage. Defaults remain SDR and existing PQ/HLG checks run unchanged.
6. [ ] Expose opt-in CLI/FFI gain-map options and route existing MCP HDR JPEG requests to this output, with explicit rejection of incompatible transfer/bit-depth/color-space/original/watermark/enhancement combinations. Add focused positive/negative host tests and schema/preset coverage; do not change unrelated export paths.
7. [ ] When slot granted: targeted release core gain-map and HDR/native tests; focused CLI/FFI/MCP tests; appropriate export/sidecar regression suite and strict Clippy. Record exact commands/results/OS and observed whole-frame diagnostic. Review final diff, then commit narrowly and report head; no main merge/push without coordinator.

## Review focus

- ISO flags/rational values and MPF offsets must associate the actual auxiliary image after metadata insertion and byte-budget search.
- SDR-only readers must see a usable sRGB base; native HDR tests must check actual pixels, not metadata presence or peak alone.
- Explicit metadata privacy/keyword overrides must survive the container changes.
- Unsupported host requests must fail before output, never silently export SDR or ignore a transfer.
- Lossy boundaries are not covered by the five-center tolerance; document their measured discrepancy separately.

Lightweight API red reproduced before implementation using rustc metadata-only against accepted cached export artifact; E0560/E0609 missing gain_map, exit 1. Exact command/log: `/tmp/tessera-iso-gainmap-control/restoration-api-red.log`. No heavy build launched.

## Source preparation checkpoint (not a validation pass)

Core routing and ISO+MPF-only encoder restored; descriptive metadata continues through the accepted native/XMP pipeline. CLI `--gain-map`, FFI JSON `gain_map` (default false), and MCP `hdr:true` JPEG route are wired. No dependency or Swift UI changes. Tests cover independent MPF/ISO extraction, unchanged PQ-reference patch centers, actual native SDR/HDR pixels, ISO/MPF loss controls, no-clobber/cancellation/budgets, invalid headroom/options/AI masks, 1/2/4-stop resize/sharpen, policy readback, long XMP and a gain-map metadata source without duplicate ISO/MPF. The existing cached CLI reports the expected unknown `--gain-map` argument (exit 2), preserved in `/tmp/tessera-iso-gainmap-control/restoration-cli-red.log`.

`rustfmt` on changed Rust files and `git diff --check` pass. Full fresh core/host tests, strict Clippy and relevant regression gates have NOT run; coordinator holds the heavy build slot. Source remains uncommitted. The initial gate will be targeted release `gain_map` and `gain_map_hosts`, with `--nocapture` preserving whole-frame diagnostics, then relevant HDR/native checks.

## Native-reader disposition checkpoint

The frozen A core gate is **4 passed / 1 failed**, and the original ImageIO four-stop assertion remains unchanged. The failing case decodes to ~7.98376 with reported headroom8 although independent ISO/base+gain reconstruction reaches16. Default context EDR target is already0; explicit0, luma-scaling off and allowed-float output yield identical raw ImageIO providers. These negative probes justify no helper repair or encoder change.

A separate software Core Image diagnostic reconstructs the same1/2/4-stop files to2/4/16 using both default HDR expansion and documented `imageByApplyingGainMap:headroom:`. A separately named supplemental test is prepared in gain_map.rs, with a test-only Objective-C reader compiled once via installed xcrun, CPU/software rendering, RGBAf extended-linear-sRGB working/output spaces, finite/opaque readback, unchanged4% dark/bright patch bounds and missingISO/MPF controls. It is **not yet run**, is not a replacement for ImageIO acceptance, and cannot convert that retained failure into a full package pass.

Next assigned slot may independently validate the supplemental test and CLI/FFI/MCP gain_map_hosts targets. Host gates are independent correctness checks after the explicit coordinator disposition; they do not waive the original native-reader failure. No production changes are warranted by the diagnostic. Full package/main integration remains held pending explicit acceptance disposition and evidence. MachineB heavy slot belongs to its coordinator and must not be used.
