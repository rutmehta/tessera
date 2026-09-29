# Opaque captured-CFA owner implementation qualification (GREEN)

Exact `d908374fd530d925e440bb21df209e60e694209b` (source-approved implementation over RED/API `c0993358`). Runner `retry-v2` = original runner plus mode-preserving artifact copy and explicit launch-error recording only; no source, test or oracle change.

All eight phases passed, serialized, stop-on-first-failure:

1. compile: Release tests, locked, jobs 2 — direct exit 0.
2. green: five protocol tests 5 passed / 0 failed / 0 ignored.
3. api: downstream positive compiles; eight negatives fail with intended primary codes (Clone/conversion/Default E0277, metadata/opcode mutation E0596, parts/sample getter E0599, private fields E0451).
4. full raw-decode: 98 passed / 0 failed / 4 ignored (two pre-existing native qualification tests, two new family tests run separately).
5. family-internal: Sony ARW, Fuji RAF, Nikon NEF, Canon CR3, DNG each EXERCISED once; owned samples match existing direct decoder. 1 passed.
6. family-external: same five families through public opaque API survive capture cleanup. 1 passed.
7. strict (clippy) exit 0. 8. fmt exit 0.

Source freeze equal before/after every phase (9,231 Git entries). Independent read-only verification: PASS (see INDEPENDENT-REVIEW.*), 221,544 freeze entries rehashed, retained binaries/rlibs match, fixture SHA256 match authority.

The original first `02-green` attempt is preserved unchanged inside the tarball: it failed to launch (PermissionError, retained copy lost execute mode) before any test ran — a harness setup failure, not a product result.

Scope: component qualification only. No rendering, public normalization/conversion API, ICC authority, or decoder memory-bound claim.

Executed by Machine A Claude coordinator after Codex handoff, 2026-09-29.
