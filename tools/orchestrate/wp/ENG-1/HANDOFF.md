# ENG-1 source preparation

Assignment: main b07993a1. Owner: Codex Machine A; Claude coordinator owns merges. Source-only preparation is preserved alongside other engine source plans on codex/perf-1-styles; implementation must use its own codex/eng-1-conditioning branch based on current main.

Accepted/status mailbox publication: 9391d593-839a-4d71-b5be-0c47fef12b43. Publication succeeded; peer receipt and wakeup unverified. Git note on b07993a1 preserves the acceptance.

Independent source investigation and validation planning completed; no implementation, tests, builds, GPU runs or Develop acceptance. Declined B5-32 numerical commit 635f69d8 was not taken. See SOURCE-PLAN.md, INDEPENDENT-REVIEW.md and VALIDATION-PLAN.md.

Review rejected a naive total-gain denominator floor: below epsilon, tiny nonzero changes attenuate signed RGB while exact no-change is preserved, introducing a discontinuity. Next action: author policy-independent continuity/sensitivity RED fixtures, covering both GPU shaders and zero/sign crossings, then select a conditioned update using measured results. No epsilon or product formula is established. Retain exact default Develop goldens and existing tighter operator tolerances; require all-pixel absolute 0.01 Camera Raw evidence including the 24 MP stress case before claiming the requested bound. Runtime remains held by external batch16 parent 18601 through Swift completion. No competing compilation may start.
