
## Initial non-discriminating baseline attempt

The first deterministic-test attempt ran with only `ReviewCurrentPreview` temporarily reverted to the old immediate `barrier.result()` call while preserving loader/test seams. It **passed 1 test, direct exit 0**, but is not valid RED evidence: gate-count creation did not prove the second evaluation had returned, so it could run after the first drain. The exact modified/restored source hashes and raw log are retained here. A later test-only revision at commit `634b7e68` waits for either an actual preceding-drain waiter or a fail-closed completed gate before releasing the held worker, then requires a second render and cache delivery. The revised baseline has not yet been run.
