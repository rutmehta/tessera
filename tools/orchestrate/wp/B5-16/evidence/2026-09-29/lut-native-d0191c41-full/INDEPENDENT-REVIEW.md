# Independent full Swift d019 verification

APPROVED for the recorded full automated Swift gate at d0191c41859171dc28ef4114891ed4a4c51084b6. Read/hash verification only; no compiler, test, GUI or GPU workload rerun.

Independently recomputed all19 VALIDATION-SHA256 payload hashes and all8,396 source hashes against immutable Git blobs. Source sets match exactly. Before/after source, HEAD, four ignored FFI artifact hashes and Sony fixture hashes match; those artifacts and fixture still rehash correctly. Runner/imported runner/oracles/baseline qualification-input maps match before/after and actual recorded files.

Direct full-test exit0. Parsed raw log independently:741 XCTest pass lines,1 skipped,0 failed =742 executed; Swift Testing5passed. All155 required exact-name pass oracles occur exactly once and agree with required-results.json, including both actual Sony offline-thumbnail and offline-library/render/save/reopen/reconnect workflows. Sole XCTest skip is the explicit opt-in generated20k Library measurement. This is not a strict test-harness build: original weak-variable/Sendable warnings remain recorded. Missing Lens Blur model warning does not qualify that feature.

Preserved full-test relink binaries rehash to the after-map identities:
- Tessera:7dc0d8e659af5057cb27a4a63d099f9debf55862ec4a7128317341f33850c112
- TesseraPackageTests:e0d2d92507a871bbbb9031bf2ad28acfa7d36f29985c893ebaa968de73456516

These are full-test outputs; the earlier strict executable and GUI package are distinct and are not relabeled by this review. No new GUI/VoiceOver acceptance, source mutation or merge. Machine-readable verification is /tmp/tessera-d019-full-independent-review.json; reproducible read-only verifier /tmp/verify-d019-full.py.
