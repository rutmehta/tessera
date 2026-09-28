# Recovery retry status validation

This source-only follow-up addresses a stale Library-navigation status after a failed Develop close is explicitly retried successfully.

## Results

- **RED:** tests-only commit `76fd1a0c94c63f1496433c661a23db6c37fb6393`, product baseline from `2760ebeb`. The single regression test failed at the expected final assertion: the editor had closed and the grid destination committed, but `statusMessage` still contained `Finish saving the photo before leaving this workspace`. Direct exit 1. This is a behavioral failure, not a compile or fixture failure.
- **GREEN:** tested source HEAD `7135ea13ad1174dc18fd437585eaa1103dcca74e`. `DevelopRecoveryAdmissionBehaviorTests` executed 19 tests, 0 failures, direct exit 0. This includes the stale-message clear test and a held-close control that publishes an unrelated newer status after retry starts and verifies it survives.
- The checked-out repository originally had archive SHA-256 `19f9f5487752e2588c6febe99d126905cad7d9986285d2aebf633dfaeba2b427`. Tests used the exact current archive from the validated workspace-redesign checkout: `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03`, with matching generated Swift binding and C header hashes. The original local archive was restored after testing; see `archive-restored-final.txt`.
- Source and generated binding/header hashes are recorded immediately before and after each run. The archive was the only intentional change to the ignored package-local build input during a run; it was restored byte-for-byte afterward. No user app or GUI was used.

Each attempt directory contains its exact command, raw log, direct exit, tested HEAD, and source hashes. `SHA256SUMS` covers all payload files except the manifest itself.
