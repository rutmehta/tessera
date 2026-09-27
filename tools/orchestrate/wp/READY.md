# Branches ready for the coordinator to merge (machine B)

- (merged) wp/B5-ui — one branch containing B5-01 (FFI DocumentSession), B5-02 (document mode UI), B5-03 (engine wiring), B5-04 (tools), B5-05 (filters), the Swift 6.2.4 AssistController fix, and their board cards. Verified: cargo test tessera-ffi/filters/compositor ok, clippy/fmt clean, swift test 155 XCTest + 5 Swift Testing, xcodebuild succeeded. Merge this single branch.

- (merged) wp/B5-06 — B5-v fixes (⌘E from grid, Save As name field, §V test count) + Properties editors for all 23 adjustment types. Merged with main 923aad8; cargo tessera-ffi ok, clippy/fmt clean, swift test 224 + 5, xcodebuild ok.
- (merged) wp/B5-08 — persistent alpha/spot Channels panel (document/channels.rs), Quick Mask, Save/Load Selection on persistent channels. Based on main 923aad8; cargo compositor/selection/psd/tessera-ffi ok, clippy/fmt clean, swift test 193, xcodebuild ok.

- (merged) wp/B5-09 — Remove tool (stroke/selection, Auto/PatchMatch/LaMa), Content-Aware Fill, reviewed Remove Distractions, Neural Filters panel (document/retouch.rs builds masks engine-side). Merged with main 9a6f25f; cargo filters/ml-filters/tessera-ffi ok, clippy/fmt clean, swift test 240 + 5, xcodebuild ok. ACCEPTANCE §Z steps 320–333.

- (merged) wp/B5-07 — Layer Style inspector (11 effects, schema-driven editors), Global Light, copy/paste/clear styles (document/styles.rs), CPU-composite fallback for styled layers in document/render.rs. Merged with main c1f95e1; cargo compositor/psd/tessera-ffi ok, clippy/fmt clean, swift test 254 + 5, xcodebuild ok. ACCEPTANCE §AB steps 300–314. Includes the Swift 6.2.4 ExportWatermarkViews fix (identical to main).

- wp/B5-10 — Type tool (point/area text, canvas caret, NSTextInputClient/IME), Character/Paragraph inspector, convert to pixels, source-preview cancel (document/text.rs), 1440-pt inspector clipping fix. Merged with main 1cd02b3; cargo typography/compositor/psd/tessera-ffi ok, clippy/fmt clean, swift test 295 + 5, xcodebuild ok. ACCEPTANCE `## B5-10.` steps 340–359. Needs on-screen: Japanese IME (351), caret/box-resize feel.
