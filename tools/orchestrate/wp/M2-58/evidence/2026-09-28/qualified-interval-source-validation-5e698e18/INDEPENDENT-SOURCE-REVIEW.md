# Independent source review: qualified interval analyzer

Exact checkpoint reviewed: 70e48fe5251461dd4a277bc71c7bc9f3ccb11a00 in workspace-redesign. Review only; no Python/native/Swift execution by this reviewer. Owner reports 14 Python tests passed. The checkpoint changes only app_timing_visible.py and its test file; handshake/app/probe wiring is not included or cleared.

Source conforms to actual trace shape: DevelopController input records session/input/backend without frame generation or level; job_dequeue and callback_enqueue supply frame generation/level and input; FrameTiming carries those identities to drawable_presented. The qualified analyzer uses those frame events, matches the presentation input, and accepts legitimate refinement/redraw chains while selecting the earliest positive actual presentation for each input. Its p50/p95 derive only from accepted interval chains, not the legacy full-trace summary. It requires zero drops, one ordered marker set, nonce-bound ready/permit identity, exactly 121 consecutive ordered input IDs, completion after all input times, final-input presentation by measurement_end, at least 100 causal presentations, and bracketing foreground/window samples with gaps no greater than 250 ms. No analyzer algorithm blocker found for this isolated scope.

Two test discrimination corrections requested before using the new assertions as evidence:

1. The wrong-presentation-input fixture removes the ordinary chains for inputs 1 through 23, leaving 98 ordinary joins plus at most one refinement input, or 99. It therefore fails the 100 threshold even if the incorrect input is accepted. Its second refinement redraw also retains input1. Remove only inputs1 through22, show the unmutated refinement control qualifies100, then mutate BOTH refinement presentation input IDs so the corrected analyzer rejects with99. This distinguishes the intended join check.
2. The refinement fixture has an earlier 10.13 presentation for input at10.0 and later baseline10.3, but only asserts count. Assert the selected first-input latency is approximately130ms to reject a300ms/non-minimum implementation.

Visibility remains sampled evidence, not proof of continuous occlusion-free display or exact detail pixels. The runner integration must pin process launch identity, window/session and nonce, provide samples in the same host clock, use the qualified result metrics, and fail when ready/permit/drain or interval qualification fails. Those future source paths remain to be reviewed. No P01 result or P11 acceptance is claimed.

## Frozen application protocol follow-up

Exact final source reviewed: 9bf95afcf5dc16bf3e1b45eba49c9da91d594f7d, including protocol implementation 5174d90e and ready-identity follow-up. Source-only clearance for coordinator-controlled compiler/test validation; no reviewer execution and no GUI or measured interval acceptance.

Swift TimingVisibleHandshake now serializes snake_case fields matching the runner, binds nonce/PID/bundle ID+URL/launch date/window/session, and ignores only the timestamp when comparing run identity. The state machine permits one finite ordered start, rejects stale/wrong/duplicate start, enforces waiting and measurement deadlines, and permits one ordered end. The app warms on a positive actual presentation in the captured controller session, atomically publishes ready, waits boundedly for its permit, emits measurement_start before inputs, runs all 121 scripted Exposure calls, validates exact consecutive input IDs, then drains the final input to a positive actual presentation before measurement_end. Current controller and regular active/key/visible window identity are rechecked at waiting, each input and drain boundaries; failure emits qualification_failed and stderr reason. The analyzer remains the authority for complete causal-chain/minimum-count qualification.

Normal/background behavior is retained: AppModel's new branch requires both --timing-visible and --timing-selftest, the previous background loop/flush and selftest_timeout remain, and existing conflicting host-mode handling terminates before diagnostic work. TesseraApp changes here are comments. Lease, saved recipe/histogram, generated bindings and normal renderer paths are not changed by the protocol checkpoint.

The source still describes scripted AppModel.setAdjustment plus explicit flushPending, not an OS mouse/slider gesture. Window evidence is sampled; current guards are not continuous pixel visibility. No P11 detail matcher/capture/display claim is added. Root independently reviews Python/typed-test coverage and will authorize any build; this note is not evidence that Swift compiled.

## Compiler evidence and narrow correction

The subsequent focused Debug compile at9bf95afc found two test call sites with `time` placed after `bundleURL`/`launchDate`, contrary to the local Swift helper signature. Source preflight did not catch this. The first occurrence was tool/session-observed only (no durable raw file); a second preserved attempt has focused-unfixed.log and direct exit1. Both occurred before test execution. No passing test claim applies to either attempt.

Exact correction5e698e1836c2cf050b00d8ae7c8f8f93c3161edb only reorders these two test call arguments; all test assertions, data, and product source are unchanged. Independently inspected diff confirms the correction. Saved subsequent focused Debug logs contain27 distinct passing tests/direct0: protocol5, host launch-mode4, PerformanceTrace3, detail scheduling9, mailbox2, Develop4. Python17/direct0 is separately saved. These focused tests are a subset of any later full suite and must not be added to its count.

Both6,425-entry pre-correction and pre-Release manifests differ only at the corrected Swift test file. Independently verified1,538 tracked Rust/Swift/header/Metal/Cargo/Package/protocol-Python source blobs against Git5e698e18. The initial generated-bindings.sha256 is empty and is not usable binding evidence. The later complete release-generated-before.sha256 and independent current-file hashes confirm archive07d924df, Swift0efe51ef and header61dbdf84. Full Release evidence remains separate until its final direct exit and after-input freeze are available.

## Final saved validation audit

At unchanged5e698e18, accepted full Release attempt is swift-release-full-exit-captured.log with durable child direct exit0. Independently parsed666 XCTest executions:665 distinct passes, one skipped library measurement, zero failures; Swift Testing5 passed. All27 focused Debug names are a subset of these full-suite cases. Repeats and focused counts are not additive.

Two preceding full Release logs report the same completed suite counts but are not accepted direct-exit gates: the first tool session expired before status collection; the second wrapper assigned zsh reserved `status`, yielding a wrapper error/direct1 reported by coordinator, with Swift child status unknown. Their raw logs remain preserved. Third attempt used child-status capture; owner command/toolchain note documents the distinction.

Release before/after6,425-entry manifests are byte-identical SHA2563eb6e8474432cf96e9c18f576d0a8eb8d8f2affce19e0a0b32e4d9046028373e, and complete generated/header/archive manifests are byte-identical and independently rehashed against current files. The saved initial generated-bindings manifest remains empty and has no evidentiary value. No renderer/app GUI/capture/native rebuild was performed by this reviewer. Source tests pass; visible qualified interval and P11 acceptance remain unmeasured.
