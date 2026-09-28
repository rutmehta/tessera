# Proxy decision reuse Task2 functional qualification

Final clean `7faea4b8e7a3bdc46afcf794a9e4dc5c6710be6c`. Source reviewer approved059a45d1; adee078f applied rustfmt only; final7faea4b8 applied the independently approved non-test infallible-destructuring binding correction. No behavioral repair or tolerance change.

Final exact-source gates: strict19 direct0; pure20 all22pass/0ignored; opt-in Engine21 all8pass/0ignored; full native22 direct0 (443 passed, 25 ignored across all suites,0failed); fmt23 direct0. Full FFI unit subset215passed/10ignored; the8new opt-in groups were explicitly executed separately, not silently counted as full-suite passes. Some unrelated existing opt-in/benchmark tests remain ignored.

All phase source/fixture/runner before-after freezes equal,8,646 source inputs. Explicit read-only Sony SHA256 `bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8` unchanged. This preservation claim is for the explicit Sony fixture; full-suite tests may read other existing fixtures through their own discovery, which this runner did not independently inventory.

Preserved history:01 missing-instrumentation RED;02–11 instrumentation controls and missing-initial-entry RED (including06fmt failure);12compile passed;13fmt1;14–17 formatted functional GREEN;18strict101 for one production-only Clippy lint;19–23 finalGREEN. Full raw logs/warnings/direct exits and per-attempt commands remain, with distinct pinned runners.

Eight actual Engine groups include all six original contract bodies, explicit uncached-selection control and persisted external-profile/LUT bypass. Tests prove validation before lookup, same-key reuse for CPU/Metal SDR/EDR, full identity transport, rebuild/edit misses, override/external/geometry/device bypass, active-editor/failed-open handling, and fresh Renderer lifetime. Controlled measurement samples are not performance or timed GPU dispatch evidence.

No main merge, public FFI/default-source change, archive/Swift qualification, benchmark or speedup acceptance. Follow-up actual IOSurface/lifecycle/fidelity and fixed paired baseline comparison remain pending independent source review and separate runtime grant. Runtime lane released after23.

See FINAL-CHECKPOINT.json for exact manifests and gates. cache-candidate.patch captures baseline3614 to finalcandidate source; review files preserve bounded source approvals.
