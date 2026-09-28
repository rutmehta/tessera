# EXP-45 isolated HDR-statistics option review

2026-09-28. Independent source review and later saved-artifact audit only. This reviewer does not compile or execute native probes. Root authorized Luna a separate comparison on three original80x16 fixtures, default versus nested `kCGImageSourceDecodeRequestOptions: {kCGComputeHDRStats: YES}`, after phase8 publication. No other decode/render/options or product changes are in scope.

The prior completed geometry review remains frozen in `/tmp/tessera-exp45-imageio-path-review.md`. This file does not replace the original A4pass/1fail result or establish product acceptance.

## Documented contract

Installed Xcode SDK CGImageSource.h:256–257 declares kCGComputeHDRStats. Apple demonstrates the key inside kCGImageSourceDecodeRequestOptions in https://developer.apple.com/documentation/coregraphics/adopting-advancements-in-hdr-image-rendering . Its documented purpose is to calculate HDR metadata, not to request a numerical decoder headroom. Source review must verify that this is the only experimental option change and that all other decode/render parameters remain at the verified phase8 defaults.

## Initial review status (superseded by completed audit below)

At initial review, source/runner hashes and execution were pending. The completed audit below records the final outcome.

## Exact probe source review

Reviewed `/Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/stats-option-80x16/probe/imageio-probe-stats.m`, SHA256 `5cfcb528ba3ea5da4698999cc6c1f46c43c753853fe4f370554dedd3213df80b`, by diff against phase8 source `ffefb2e681e51763f81f48b2b57b69e55dae0b1d4042476365fc227945968ab3`.

Only changes are a conditional nested Stats=YES dictionary on HDR requests when PROBE_HDR_STATS is present, and a JSON flag reporting whether that dictionary was added. SDR decode, existing decode/cache/float choices, provider/profile capture, linear rendering, sampling and post-render source-property introspection are unchanged. No guessed or Beta API is introduced. The conditional checks environment presence, not its textual value, so the runner must unset the variable for default cases rather than assign '0'. Enabled cases should set it explicitly and validate the reported SDRfalse/HDRtrue flags. No source blocker for compile was identified. Runner review/native execution remain pending at this checkpoint.

## Exact runner source review

Reviewed `stats-option-80x16/probe/run_stats_comparison.py` first at `32dfca0c8d0f79ea314947227914920e142db3a3a06ce5c638db2eea47a065b7` and final at SHA256 `8d8de47680f0687f658893826779e39d2b54ad14d95d374b9b3ba1e501d25839`. Requested and verified preflight additions: assert exact input hashes against phase8 provenance, check default HDR headroom/peak before the paired Stats process, and describe the controls accurately as separate processes.

Final runner uses exactly three retained80x16 inputs and two variants each. Every process gets a fresh output directory (`exist_ok=False`), inherited option overrides are cleared, and PROBE_HDR_STATS is absent for default and present for enabled. Returned JSON must confirm SDRfalse in both variants and HDRfalse/true as selected. Provider and returned ICC files are checked against probe hashes/lengths; float output size is80*16*16; source properties must parse. Default results are compared with phase8 and paired changes are reported without asserting a desired Stats outcome. Probe measurement path remains unchanged except for the documented HDR option. No source blocker remains within the authorized six-process scope. This is review only; native results remain pending.

## Independent completed saved-artifact audit

Read final `stats-option-80x16/stats-comparison-manifest.json` and all six process output directories, without invoking a native executable. Verified direct exit0 for each process and exact retained input hashes against phase8. Final provenance: probe source `5cfcb528ba3ea5da4698999cc6c1f46c43c753853fe4f370554dedd3213df80b`; binary `f7613645cb60c4866d8c332075f3badec4794cb3c3fb0532264082812d76592b`.

All12 drawn RGBAf buffers have exactly20,480 bytes, all finite channels, and exactly opaque alpha. Independent scans reproduce the reported minima, maxima, means and sample values. Every provider and ICC file matches its recorded length/SHA256. For each input, DEFAULT and STATS-enabled SDR and HDR provider bytes, drawn-float bytes and ICC bytes are exactly identical. Ordinary source-properties plist bytes are also exactly identical within every pair and match their recorded hashes.

All three default runs' SDR/HDR provider bytes and drawn-float bytes match phase8 exactly. Their dimensions, extrema/means, headroom, bit layout, context headroom flags/values and sampled RGBA records also match phase8 exactly. No repeated native call was needed for this comparison.

The source predicate is `request == kCGImageSourceDecodeToHDR && getenv("PROBE_HDR_STATS")`; the saved records verify compute_hdr_stats=false for every SDR decode, false for every default HDR decode, and true for every enabled HDR decode. Thus the option was actually requested on the intended path. An unchanged output does not prove the implementation did or did not perform extra internal statistics work.

| Input | Default / Stats HDR headroom | Default / Stats linear RGB maximum |
| --- | --- | --- |
| Uniform80 |16 /16 |15.951576232910156 /15.951576232910156 |
| Split80 |8 /8 |7.983762741088867 /7.983762741088867 |
| Original A80 |8 /8 |7.983762264251709 /7.983762264251709 |

The documented HDR-statistics opt-in does not change the observed provider representation, reconstructed pixels, generated profile or source properties for these three fixtures on this host. This eliminates that public option as a resolution of the observed discrepancy in this bounded experiment; it does not explain the internal implementation or establish an Apple defect. Original A4pass/1fail remains unchanged; no test waiver, encoder correction, or product acceptance follows. No further HDR native experiment was performed or authorized by this reviewer.
