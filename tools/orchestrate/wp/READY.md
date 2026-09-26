# Branches ready for the coordinator to merge (machine B)

- wp/B5-ui — one branch containing B5-01 (FFI DocumentSession), B5-02 (document mode UI), B5-03 (engine wiring), B5-04 (tools), B5-05 (filters), the Swift 6.2.4 AssistController fix, and their board cards. Verified: cargo test tessera-ffi/filters/compositor ok, clippy/fmt clean, swift test 155 XCTest + 5 Swift Testing, xcodebuild succeeded. Merge this single branch.
