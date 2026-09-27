# Branches ready for the coordinator to merge (machine B)

- wp/B5-ui — one branch containing B5-01 (FFI DocumentSession), B5-02 (document mode UI), B5-03 (engine wiring), B5-04 (tools), B5-05 (filters), the Swift 6.2.4 AssistController fix, and their board cards. Verified: cargo test tessera-ffi/filters/compositor ok, clippy/fmt clean, swift test 155 XCTest + 5 Swift Testing, xcodebuild succeeded. Merge this single branch.

- wp/B5-06 — B5-v fixes (⌘E from grid, Save As name field, §V test count) + Properties editors for all 23 adjustment types. Merged with main 923aad8; cargo tessera-ffi ok, clippy/fmt clean, swift test 224 + 5, xcodebuild ok.
- wp/B5-08 — persistent alpha/spot Channels panel (document/channels.rs), Quick Mask, Save/Load Selection on persistent channels. Based on main 923aad8; cargo compositor/selection/psd/tessera-ffi ok, clippy/fmt clean, swift test 193, xcodebuild ok.
