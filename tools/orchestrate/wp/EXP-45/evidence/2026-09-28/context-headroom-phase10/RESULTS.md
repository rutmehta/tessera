# EXP-45 CGContext target-headroom phase 10

## Outcome

This diagnostic changed only the EDR target on the destination `CGContext` used to draw the same ImageIO-produced `CGImage`. It did not alter ImageIO options, source JPEGs, returned profiles, renderer, sample points, or acceptance thresholds.

At target 8, the uniform-gain control's HDR draw peak changed from `15.9515762` to `7.9886270`; at target 16 it returned to `15.9515762`. `CGImageGetContentHeadroom` stayed 16. For both the original A fixture and the split-gain reference, default/8/16 all drew the same floating-point pixels (peaks `7.9837623` and `7.9837627`) and reported content headroom 8. The setter succeeded and the getter returned the requested 8/16 values in all non-default cases. SDR output was unchanged for all fixtures.

All provider bytes remained identical across the three target settings for each source. The returned ICC profiles also matched across those settings. Against phase8, the A and split HDR ICC files differed only in header creation-time bytes 24–35 (for these captures, offsets 31, 33, and 35); length, declared length, all other header bytes, tag table, and every tag payload were identical. Raw ICC files, raw hashes, timestamps, and timestamp-excluded hashes are retained. The uniform HDR ICC matched phase8 byte-for-byte.

This confirms that the target headroom can affect the destination draw/tone-mapping path for the uniform control. It does not explain the A/split ~8 ceiling: requesting 16 did not lift either. The remaining question is upstream of this context setting and still includes ImageIO provider decoding versus source-profile interpretation. No decoder target was set or inferred.

The original ImageIO acceptance failure remains unresolved and unchanged. This phase is diagnostic only; it does not turn the original failed case into a pass.

## Attempts and evidence

- `evidence/attempts/attempt-01/`: compile exit 0 and one successful A80 default native process exit 0, followed by a runner assertion on whole-ICC SHA. The preserved ICCs show only the creation-time bytes differed; no target8/16 process ran.
- `evidence/attempts/attempt-02/`: runner exit 1 before native process launch because a copied binary lacked its executable bit (`PermissionError`). It contains no native direct exit or measurement.
- `evidence/attempts/attempt-03/`: completed nine per-file/per-target processes; each native direct exit and the runner direct exit are 0. The runner first qualifies each default case against phase8, then runs target8/16. It records before/after source, runner, binary and fixture hashes, raw provider and ICC data, ImageIO float outputs, profiles, warnings, commands, and per-process exits. The executable binary itself remains on the BetterSSD validation volume and is represented here by its SHA-256 and mode rather than copied into Git.

The independent phase10 payload audit reports 18 finite RGBA-float buffers (each 5,120 float values, alpha exactly 1), and confirms phase8 default drawn hashes, provider hashes, and all nine process exits. See `evidence/independent-audits/` and the raw per-run JSON/manifests.

## Reproduction

Source checkpoint: `b501cc96b50562f4b738465b27db12427cf6446b` on `codex/exp45-context-headroom`; base was `efa316f244f7475297883d63648fde49ac12bf6f`. Probe SHA-256: `709fc46186e8a2907466fef052fb4109ad7af3060c5067f7d9f18a4ccae23529`; runner SHA-256: `562d6775fb63d3ce5b5030c85390a2541a9668db05c8645540451027b1153062`; probe binary SHA-256: `5a0ca1f883de5aaabca91691a2666ad016d80fff150b20660ded7d0932b580c0` (executable mode `0755`). The exact commands, source freezes, ICC copies, fixtures, raw attempt outputs, and all per-run logs are retained under `evidence/attempts/` using relative paths. `SHA256SUMS` lists every packaged payload with a path relative to this phase10 directory and omits itself.

No application product code or acceptance thresholds changed.
