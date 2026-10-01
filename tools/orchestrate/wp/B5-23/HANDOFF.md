# B5-23 handoff — small follow-ups (self-test sweep doc, ⌘-click channel, B5-19b release note)

Branch `wp/B5-23` (Machine B), base origin/main 8d3996f7. Swift + docs only: no Rust, `board.json` or `Cargo.lock`.

## Commits

- `93c6b6be` cherry-pick of e828b87f (wp/B5-selftest-window): `tools/orchestrate/wp/B5-selftest-window/SWEEP-2026-09-29.md`, clean.
- `2847c74e` RED: `DocumentChannelsTests.testRowClickHighlightMatchesThePaintTarget` pins ⌘-click =
  `ChannelRowClick(highlight: nil, retarget: false, load: 7)` (also ⌘ on a colour row → nil). RED is a compile
  failure: `load` changes from `Bool` to the channel id to load.
- `d6f25b83` fix + release note.
- this handoff.

## ⌘-click on a channel row (B5-17d review nit, Machine A)

Photoshop: ⌘-clicking a channel thumbnail loads it as a selection and does not change the target channel. Before,
Tessera highlighted the ⌘-clicked row while paint stayed on the layer, so the highlight lied. Now
`ChannelsPanelModel.click(_, command: true)` returns `load: <id>` with `retarget: false`, and
`ChannelsPanel.click()` updates `selectedChannel` (the highlight) only when `retarget`, together with
`DocumentTools.targetChannel`. The highlight therefore always matches the paint target. ⇧ / ⌥ / ⇧⌥ still pick
add / subtract / intersect. `ChannelRowClick` doc comments and the `click` comment were updated.
Files: `apps/mac/Sources/TesseraCore/Document/Channels/DocumentChannelsBackend.swift`,
`apps/mac/Sources/Tessera/Document/Channels/ChannelsPanel.swift`, `apps/mac/Tests/TesseraCoreTests/DocumentChannelsTests.swift`.

## Release note (B5-19b)

The repo has no RELEASE-NOTES / CHANGELOG / docs/release* file and no user-facing Photomerge doc page, so the
note went under a new "## Release note" section in `tools/orchestrate/wp/B5-19b/HANDOFF.md`: the stack budget is
half of physical RAM at ~50 B/px, capped at 200 MP (≈85 MP on 8 GB, 171 MP on 16 GB), so 8 GB Macs may refuse
stacks they used to accept.

## Gates

- `apps/mac/build-ffi.sh`: exit 0.
- `tools/orchestrate/swift-gate.sh`: 837 XCTest tests, 3 skipped, 0 failures; SWIFT GATE OK.
- Rust gates not run (no Rust changed).
- Not verified on screen (no screen / focus taken).
