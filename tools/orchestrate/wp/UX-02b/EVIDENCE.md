# UX-02b validation evidence

The final focused gate ran on the source state represented by `evidence/ux02b-group-validation-freeze.sha256`:

```sh
swift test --package-path apps/mac --scratch-path /Volumes/betterSSD/tessera-ux02b-review-resume --jobs 2 --filter 'ReviewResumeStoreTests|AgentReviewQueueTests|AgentReviewNavigationStateTests|AgentReviewOwnershipTests|AssistTests'
```

The test process exited 0: 47 selected tests, 0 failures. The preserved transcript is `evidence/ux02b-group-validation.log`. The hash manifest freezes the Swift product/test inputs immediately before that gate. The broader preceding controller gate and its manifest are retained separately.

Earlier evidence is intentionally retained without rewriting: the test-first compile RED (`ux02b-review-resume-store-red.log`), the first store GREEN runtime failure (`ux02b-store-green.log`), the subsequent recursion crash (`ux02b-store-repaired.log`), and the later controller gate (`ux02b-controller-gate.log`). These show the progression and are not acceptance evidence. The final group-validation gate is the current passing result.

No app build, launch, GUI interaction, RAW fixture, or real provider/network call was performed for UX-02b. The gate verifies deterministic store/controller behavior with temporary generated photos and the scripted planner.

The Review empty-state sentence was updated after the focused gate to say queues resume when reopening the library; this copy-only adjustment was not part of the gate. `evidence/final-source.sha256` records the final candidate bytes, while `ux02b-group-validation-freeze.sha256` remains the exact pre-gate test freeze.
