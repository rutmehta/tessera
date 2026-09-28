# Staged filter recovery evidence — 2026-09-28

This is a focused Release validation of the AppModel staged filter/facet draft change. The tests cover composition of repeated filter edits and person-facet edits while a Develop close is pending, explicit retry after close failure, owner replacement, and cancellation of an in-flight Layers destination by a newer filter/facet request.

## Source and native inputs

- Test-only behavioral RED source: `371b6a05b174b5ee10e8582590fcc873d21ba3f9` (test blob SHA-256 `41ba67e6980ca52e5194c2a818037bc288f9e6ab755c50c22de9969c2912cc37`); AppModel baseline SHA-256 `bb3b58bbb89004009e93f6fe89aff4c4e0be08df9ec62177e9c64ff657fdf2b2`.
- GREEN source: `dd6b382c17cfc7482573b2d9d601138abc4a65ff`; AppModel SHA-256 `9906410d8cfa1d33b84d827dd06eda8da36acf7cfd7850030fa5efa56fa204e2`; test SHA-256 `41ba67e6980ca52e5194c2a818037bc288f9e6ab755c50c22de9969c2912cc37`.
- Native FFI archive was preserved: SHA-256 `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03`.
- Generated C header SHA-256 `e188c20e48663ce3c722a3ae9ee25f66b8412ddff711e55cd897063fef65e703`; generated Swift binding SHA-256 `b78ca980fa0ed7997ad3f65c17a7ea3b509758ebf466679f130702d83f25bca4`.
- The run used the existing Release scratch at `/Volumes/betterSSD/tessera-cache/swift/staged-filter-recovery-2026-09-28/scratch`, two Swift jobs, and `-enable-testing`. Commands, direct exits, raw logs, and contemporaneous source/archive hashes are copied into each attempt directory.

## Results

- `attempt-eb15-fixture-failure`: test source `eb15a6e0c62a27de54eb977d1a0e7cadbffdecb4`, direct exit 1. All nine tests stopped in setup because the fixture expected exactly two People tiles but the two-photo fixture had four unclustered face identities. No staged-filter behavior assertions ran. This is retained as a fixture failure, not a behavioral RED.
- `attempt-371-behavior-red`: test-only source `371b6a05`, direct exit 1; 9 tests, 6 failures. The fixture was corrected to use two distinct IDs from the actual indexed person tiles. Both held-Layers tests observed one stale backend dispatch instead of zero; the composition cases failed to settle at the expected combined filter/facet state. The available contemporaneous freeze is the before-run manifest; no post-run hash snapshot was recorded for this attempt.
- `green-dd6b-focused`: source `dd6b382c`, direct exit 0; 9 tests, 0 failures.
- `adjacent-dd6b`: same source, direct exit 0; 44 tests, 1 skipped, 0 failures across `PeopleModelTests`, `PeopleBridgeTests`, `LibraryTests`, and `DevelopRecoveryAdmissionBehaviorTests`.

The GREEN and adjacent runs have matching before/after source, binding, header, and archive manifests, and both checkouts were clean at capture. No full Swift suite, GUI check, or user-library test was run for this scoped change.
