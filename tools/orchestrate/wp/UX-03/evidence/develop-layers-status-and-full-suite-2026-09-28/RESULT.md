# Layers stale status and recovery admission validation

Candidate source was frozen at `44037d651e7106c31f950868050b885785bf136d` for the focused and full gates. The earlier stale-failure RED used `f8fefdf1fd0c0a9bdaf274849b65e293f7e632a4` before Resource status ownership fixes. Each attempt preserves its exact command, direct exit, source/archive freeze, and raw XCTest output.

- `stale-failure-red`: one delayed `.failed` completion after library replacement; direct exit 1 with the expected visible status overwrite (`Edit photo-0.jpg in Layers: injected layered backend failure`). Gate drain assertions passed. This exposed the `DocumentWorkspace.say` write before AppModel completion.
- `focused-41-green`: 41 XCTest, 0 failures, direct exit 0; Swift Testing discovered 0 tests. Release, two jobs.
- `full-release`: 581 XCTest, 1 skipped, 20 assertion failures, direct exit 1; Swift Testing discovered 5 passing tests. The 20 assertions were in seven test cases: one `AgentReviewNavigationTests` case (7); five `AgentReviewOwnershipTests` cases (9); and one `DevelopTests.testAppModelOpensRenderedJpegAndHeic` case (4). Other test suites completed.

The failing navigation/ownership cases included stale Review queue cursor/status and pending Develop open/mutation settlement; the HEIC/PNG preview test ended with HEIC unavailable and PNG still loading. No retry was run. The included 5-second process sample for PID 93617 was diagnostic only; it showed work in Review ownership with Rust `ml_quality::analyze`, JPEG decode and SQLite writes, so the live suite was allowed to complete.

The frozen full-run manifest includes `fixtures/raw/sample.dng`; no user images or app data were used. This is source/test evidence, not full-suite acceptance.
