# M5-31 source reuse validation

Machine A, 2026-09-27: Apple M4, Mac16,10, 10 cores, 24 GiB, macOS26.6.2
25G83. Full host/toolchain, per-run processes/load and raw logs are adjacent.

## Change

Reuse an immutable converted style source only when the child renderer dispatches
zero blocks and output-buffer identity, region and level all match. Deduplicate
only identical immutable source buffers in auxiliary preflight and packing;
effect planes stay distinct and cache accounting remains conservative. Normal
and other non-Dissolve program steps now omit their unused random seed; Dissolve
and nested Dissolve retain it. No document IDs, cache namespaces, thresholds or
benchmark timer boundaries were changed.

The inherited benchmark used duplicate zero layer IDs. It remains unchanged for
comparison; a separate timing companion uses real document operations to assign
unique IDs and revisions. Both fixtures retain cold/warm dispatch assertions.

## Validation

- Focused source guards:6pass; seed-identity guard:1pass; live-style tests:5pass.
- Full compositor release suite:326passed,0failed,14ignored. Ignored timing tests
  were separately run in six fresh processes below; the other ignored cases are
  not newly claimed.
- Strict all-target Clippy, formatting and source whitespace checks passed.
- Frozen source patch SHA256:
  `20a9271c65aa0322083b54f0ce1eb352fa2ea79d38c91f59be2e9535db5fb90f`.
- All tests/timings ran with one heavy-build/GPU slot on A. No concurrent product
  compilation or performance benchmark was authorized. Raw host snapshots are
  retained; this is not a claim of a freshly rebooted or otherwise idle OS.

| Fixture/run | CPU cold | CPU warm | Resident cold | Resident warm | Result |
| --- | --- | --- | --- | --- | --- |
| Original 1 | 164.679375ms | 142.227208ms | 48.938917ms | 9.10175ms | pass |
| Original 2 | 153.942458ms | 144.802542ms | 40.87375ms | 6.840583ms | pass |
| Original 3 | 151.669958ms | 134.21325ms | 43.386083ms | 7.399166ms | pass |
| Unique IDs 1 | 147.176833ms | 143.831333ms | 44.990791ms | 6.616084ms | pass |
| Unique IDs 2 | 156.302167ms | 151.849333ms | 41.301125ms | 6.69025ms | pass |
| Unique IDs 3 | 144.333834ms | 123.969541ms | 37.710209ms | 6.340208ms | pass |

CPU limits remain<2seconds; resident cold/warm limits remain<100ms. No timing
sample was dropped or replaced. These local results do not retroactively clear
B's prior125.855125ms failure on9f922bf. B must repeat the requested measurements
on this published candidate; app input-to-present and export performance are
separate acceptance items.

## Preserved failed guard

The initial focused run failed5pass/1fail before timing: a test hand-built two
smart children with different pixels but the same cloned cache namespace and
same tile revision. Document::new changes state.rev, while the CPU unstyled child
cache uses root_stamp. The independent child fixture now uses SmartObject::new;
real same-namespace edits still use EditSmartObject/SetMask. No production change
was made to accommodate that invalid fixture. The first failure log and initial
source patch remain alongside the successful retry. Earlier seed/fixture and
profiling attempts are described in the retained cold-profile report; noisy
full-frame profiling logs remain at their original external paths.
