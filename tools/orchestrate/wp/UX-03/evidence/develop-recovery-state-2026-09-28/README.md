Frozen source commit: 6a03d66b925a837017b46122ab825a299f7880d1
Final focused result: 9 tests executed, 0 failures, direct exit 0.
Attempt 1 was an actor-isolation compile failure before tests. Attempt 2 reached the joined-close test and hung because the test helper discarded the active semaphore when taking the gated attempt; only the owned XCTest process was terminated after capturing its stack/log. Attempt 3 fixes the test helper semaphore lifetime and passes all nine tests. No coordinator/AppModel product source was edited for the fixture hang.
