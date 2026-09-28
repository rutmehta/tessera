# Qualified interval source validation — no visible acceptance

Exact source: 5e698e1836c2cf050b00d8ae7c8f8f93c3161edb. The protocol and analyzer compile/test evidence is accepted at this source; app/GUI/capture/visible performance acceptance is not established.

- Accepted full Release: swift-release-full-exit-captured.log + matching direct.exit=0; 666 XCTest executed (665 passes, one skipped, zero failures) plus five Swift Testing passes.
- Focused Debug: 27 distinct XCTest passes across six filters/direct0, all a subset of the full Release suite. Python analyzer17/direct0 separately. Never add focused or repeat counts to the full suite.
- Initial Swift compile failure: first attempt tool/session-only; second preserved focused-unfixed.log/direct1. Neither executed a test. Fix5e698e18 reorders only two test-call arguments; product code unchanged.
- First full Release attempt: completed raw summaries, direct status unavailable after session expiration. Second: completed raw summaries, wrapper failed assigning zsh reserved status; child status unknown. Coordinator reports wrapper direct1; the wrapper error itself is described in owner notes, not a separate raw artifact. Only third attempt has durable child0.
- Complete Release source freezes: 6,425 entries, identical before/after. The earlier pre-fix freeze differs only in the corrected test file. Independent audit Git-verified1,538 Rust/Swift/header/Metal/Cargo/Package/protocol-Python blobs against5e698e18.
- Initial generated-bindings.sha256 is intentionally preserved empty; it cannot support a binding claim. Later complete release-generated-before/after files and independent current rehashes validate unchanged archive07d924df, Swift0efe51ef and header61dbdf84.

VALIDATION-NOTES.txt preserves owner-reported commands, toolchain and run history. Raw warnings/logs are retained. INDEPENDENT-SOURCE-REVIEW.md distinguishes historical source findings, correction, compilation failure and final outcomes. independent-saved-audit.json records unique test identities and direct-status distinctions. The relative SHA256SUMS covers all package payloads except itself; no executable, scratch directory, archive or app is included.

The new metrics describe a scripted AppModel Exposure sequence with explicit flushPending, not an OS mouse gesture. These tests do not provide a visible-window run, benchmark latency, actual detail appearance or P11 pass. Main M2 product acceptance remains pending its separately authorized visible evidence.
