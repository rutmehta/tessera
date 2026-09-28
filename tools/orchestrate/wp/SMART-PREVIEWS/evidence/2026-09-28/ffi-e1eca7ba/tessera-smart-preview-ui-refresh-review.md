# Smart Preview UI refresh and save presentation review

Reviewed immutable `origin/codex/smart-preview-ui` at `2ba1d95c`, specifically production fixes `dd84ed57` and `8d35e9f6`, their tests, and AppModel/menu wiring. Source-only: no builds, tests, or shared edits.

No actionable defect found in this scoped delta.

Same-photo refresh: opening captures a selection identity distinct from the status generation. It waits for the current read to drain, follows a replacement generation without starting a duplicate read, and cancels when selection changes away and back. MainActor isolation serializes the generation checks; native open remains responsible for current identity/admission. Deterministic tests cover replacement-read waiting and away/back cancellation.

Autosave: the actual Develop controller route is passed to `didSave`; proxy-save evidence is stored separately from invalidated routing snapshots. It neither authorizes opening nor triggers an automatic full-asset check. Grid/menu presentation keeps the last-synchronized-thumbnail warning through status failure. Clean ready/available or clean missing state clears that evidence. Tests cover revalidation before opening, failed revalidation, and Original saves not inventing proxy-save presentation.

This does not establish that the Swift changes compile or pass tests. Branch handoff explicitly marks those gates unrun. It also does not expand this source review into a full UI/offline startup qualification.
