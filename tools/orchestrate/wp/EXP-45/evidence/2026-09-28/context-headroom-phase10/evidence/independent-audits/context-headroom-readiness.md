# EXP-45 CGContext target-headroom readiness (source/evidence only)

Date: 2026-09-28. This is a read-only review. No checkout/source files were changed and no build, GPU, native, or GUI run was performed.

## Finding

A small, isolated destination-context experiment is still useful, but it can only test the draw/conversion stage after ImageIO has produced its `CGImage`. It cannot ask ImageIO to decode for a target headroom and cannot, by itself, decide whether the current split/A~8 result originates in provider decoding versus the image's adaptive/unnamed profile interpretation. Keep those upstream observations frozen and separately reported.

The retained probes have only exercised the default context target (0, without calling the setter) and explicitly set target 0. I found no saved run at a nonzero target such as 8 or 16. This leaves a direct gap because the current measured ImageIO split/A result is near 8 while the qualified uniform/reference and Core Image results reach about 16.

## Evidence inspected

- Current probe: `/Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/imageio-probe.m`. It creates the extended-linear-sRGB RGBAf context, reads `CGContextGetEDRTargetHeadroom`, optionally calls `CGContextSetEDRTargetHeadroom(ctx, 0)` under `PROBE_TARGET_ZERO`, reads the value again, then calls `CGContextDrawImage`. There is no nonzero-value path.
- Current phase8 output under `.../size-640x128/native/` and phase9 output under its separate retained directory report `context_target_before=0`, `context_target_after=0`, `context_target_set=false` for the ImageIO runs. The paired image/provider/ICC artifacts and hashes remain available there.
- Older A probe and manifest: `/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/probe.m` and `probe-manifest.json`. The manifest contains `default` and `target-zero` at 1, 2, and 4 stops; the zero runs set `PROBE_TARGET_ZERO=1`. The retained `RESULTS.md`/`results.json` record no changed provider bytes or drawn values from setting zero. No explicit 8 or 16 context target appears in this matrix.
- Existing interpretation note: `/Users/rutmehta/Developer/tessera/docs/coordination/EXP45-PROBE-REVIEW.md`, especially its EDR-target paragraph, correctly distinguishes destination-context rendering from ImageIO decode options.
- Installed SDK: `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/CoreGraphics.framework/Headers/CGContext.h` documents `CGContextSetEDRTargetHeadroom` as setting the target used “when rendering HDR content to the context.” It clamps values below zero to zero and positive values below one to one. Critically, 0 means “headroom unknown” and prevents tone mapping. The function returns success/failure and is available on macOS 15+.
- Apple documentation: [Adopting advancements in HDR image rendering](https://developer.apple.com/documentation/coregraphics/adopting-advancements-in-hdr-image-rendering). The API's scope is Core Graphics rendering. No public ImageIO decode-target-headroom key was found in the retained SDK/doc audits.

## Bounded experiment contract

Use a new isolated phase directory; do not amend phase8/9 or any earlier result. Keep the existing three exact 80x16 inputs and binaries/decoder options: original A, split-gain independent control, and uniform-gain positive control. For each input, use separate process/output directories for:

1. default context (no setter; expected initial/final 0),
2. explicitly set target 8,
3. explicitly set target 16.

Require `CGContextSetEDRTargetHeadroom` to return true and the immediate getter to equal the requested value. Log before/after values and setter result. Keep the current order: decode ImageIO, freeze provider and `CGImage` profile observations, then draw the same `CGImage` to the same extended-linear-sRGB RGBAf context. Do not change ImageIO options, profile assignment, renderer, sampling, source files, or thresholds. Preserve and verify source hash, provider byte length/hash, CGImage headroom, ICC/profile byte length/hash/name/model for each target pair. Save the same raw RGBAf output and compare finite pixels/global and bright-patch peaks in the existing measurement space.

Expected interpretation:

- If only destination RGBAf values move with 8/16 while provider bytes, CGImage headroom, and profile evidence remain identical, that isolates a draw/context tone-mapping effect. It does not retroactively change the default-target result.
- If the A/split measurements remain near 8 at targets 8 and 16 while the uniform control behaves as before, the context target is not the explanation for the A/split ceiling; the unresolved distinction remains upstream (provider decode versus source/profile interpretation).
- If all three sources respond similarly, it demonstrates the destination context can alter rendering but does not explain the A-specific discrepancy.

Do not call target 0 a display target: SDK documentation says it means unknown and disables tone mapping. Do not claim these targets configure ImageIO or establish the intended HDR result. Preserve the original A failure and all prior controls unchanged; this is diagnostic evidence only, with no acceptance or tolerance change.
