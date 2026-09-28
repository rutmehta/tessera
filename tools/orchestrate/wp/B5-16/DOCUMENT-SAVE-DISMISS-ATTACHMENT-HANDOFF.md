# Observed dismissal attachment correction — SOURCE ONLY / UNRUN

Request 82960a88-0725-46a1-b556-3dc1f97a2c8c validated for exact B target/expiry and accepted before work.

Trace-free branch codex/document-save-dismiss-attachment is based on e8ddabfa. Tests-first843b0416; product3d08368e. Cherry-pick these narrow commits onto A's current candidate; do not wholesale replace DocumentWorkspace or merge this older base over accepted main Layers/status/activateDocument APIs. Diagnostics are absent from this fix branch. Prior source/diagnostic branches remain preserved.

## Observed basis

A's refined e51ef7fd trace (33 focused passes) reproduced Cancel -> enabled repeat Save As with no successor. Initial probe ran before attachment and had no later update. At SwiftUI dismissal26 the captured parent actually owned the captured sheet, but observedAttachment stayed false. didEnd33 saw parent clear, yet fallback could not pass the still-active probe lease; successor37-39 queued behind the old claim. Original e8dd failure, cfdb non-reproduction, refined e51 reproduction and initial SDK compile failure remain distinct evidence. This handoff uses the exact event sequence reported in the validated mailbox; A is publishing raw evidence.

## Narrow correction

After the existing request identity guard in saveAsPresentationDidDismiss, check only state.parent and state.sheet and require parent.attachedSheet === sheet. That actual native identity match latches observedAttachment before the existing SwiftUI/native join. It does not claim detachment. The existing didEnd path must still observe that the parent no longer owns the captured sheet. Thus a retained probe cannot deadlock this positively observed native lifetime; missing probes, unrelated sheets and stale request IDs are not promoted to attachment evidence.

An @ObservationIgnored savePresentationWindow closure supplies a hidden synthetic parent in tests; nil by default, after the captured operation parent and before existing window fallback. No public API change. No new @Observable reads, fabricated notifications, timers, delay, native endSheet calls, or modifications to terminal fallback/parent loss/lease guards.

## Tests and gates

Two UNRUN tests use a hidden NSWindow subclass with a controlled attachedSheet snapshot; no actual sheet presentation:
- Capture before attachment; actual matching attachment at dismissal; still-live probe and queued successor; native end releases the claim; late/duplicate notifications and probe teardown cannot disturb the successor.
- Wrong request while actual sheet attached must not latch; matching request while unrelated sheet attached must not latch; subsequent end cannot release an unobserved live-probe claim. Unrelated sheet remains untouched; parent-loss cleanup still releases the held claim.

Only source review and git diff --check on B. Compiler, both new tests, existing regression suite and real trace-free Cancel/queued successor/Replace GUI matrix remain A gates. No acceptance claim. B workload/paused-heartbeat/desktop-writer holds unchanged; A sole main integrator.
