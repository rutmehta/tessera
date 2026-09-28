# EXP-45 destination-context headroom review

Machine A, 2026-09-28. Independent review of source `b501cc96` and portable evidence `f5893c0f`. This is diagnostic evidence only; the original ImageIO four-stop acceptance remains failed.

| Fixture | Default drawn HDR peak | Target8 peak | Target16 peak | CGImage headroom |
| --- | --- | --- | --- | --- |
| Original A | 7.983762264 | 7.983762264 | 7.983762264 | 8 throughout |
| Independent split gain | 7.983762741 | 7.983762741 | 7.983762741 | 8 throughout |
| Independent uniform gain | 15.951576233 | 7.988626957 | 15.951576233 | 16 throughout |

The setter succeeded and getters matched all requested targets. The uniform control demonstrates a draw-stage response. Target16 does not restore the A/split result; the source decode/profile distinction remains unresolved. This does not configure ImageIO decoding or change acceptance tolerances.

Root independently checked all18 float buffers (size, finite values, exact peaks), nine direct exits and per-process freezes. Astra additionally checked outer source/binary/fixture and six retained ICC freezes, all baseline comparisons, profiles and alpha. All six default SDR/HDR draws match phase8 exactly; A/split remain byte-identical across targets, as do all SDR outputs. Provider bytes are unchanged.

Attempt01 preserved a successful native default followed by a harness failure on raw ICC hash. Root, Luna and Astra independently found only header creation-time bytes changed; every tag payload remained exact. The revised comparison excludes only bytes24–35, defined as profile creation date/time by [ICC.1 section7.2.8](https://www.color.org/ICC1V42.pdf). Raw profiles, raw hashes and differing offsets are retained; actual profiles are never modified. Attempt02 failed before launch when a binary copy lost executable permission; no native result exists. Attempt03 completed nine native calls and runner/direct0. Original UNRUN preflight fixes remain distinct from executed failures.

Root verified all218 packaged payloads against committed SHA256SUMS and copied bytes. Evidence: `tools/orchestrate/wp/EXP-45/evidence/2026-09-28/context-headroom-phase10/`. No product implementation, benchmark threshold, preview or B workload changed. Further context-target retries are not selected; current work advances M2-58 correctness and visible presentation capability.
