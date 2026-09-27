# UX04 slice 1 — readable Masks component actions

The populated Masks inspector now places its Components heading above Add / Subtract / Intersect. At the minimum 288-point inspector width, the heading stays on one line and all three action labels are visible. The heading has the accessibility header trait and each menu explicitly names its operation on the selected mask. Menu contents, action closures, mode ordering, selection and undo behavior are unchanged. Production scope is only `Inspector/MasksPanel.swift`; no shared theme, Document or navigation edits.

## Validation and preserved failures

- Baseline `6e1e7b8`: real generated JPEG, real Develop session and selected linear-gradient mask hosted in the production PanelSection/MasksPanel. Background windows cover 288 and 380 points in light and dark. No user-app activation or termination; user-opened Tessera PID 92265 remained running after the gate.
- RED: 1 test, 7 assertion failures. Six identify the real 288-point defect: wrapped Components and truncated Subtract/Intersect in both appearances. The seventh is an OCR artifact: the already-visible Add and chevron read as `Add-`. All 380-point assertions passed. Original log and four screenshots retained.
- Intermediate: the small layout fix rendered all labels correctly. ThemeLint passed, but three Add assertions failed because Vision read the adjacent chevron as `Addv`. Log and captures retained. Visual inspection confirmed full Add text, not a layout failure.
- The test then accepted either the exact full label or that full label followed by a literal `v` chevron. Language correction remains disabled; truncated prefixes such as `Sub...` cannot satisfy the assertion. The six original layout failures remain detectable.
- Final focused release gate: **2 XCTest tests, 0 failures**, 3.194 seconds; build 24.24 seconds. `MasksPanelLayoutTests` and `ThemeLintTests`. All four final captures visually inspected: full single-line Components, Add, Subtract, Intersect at both widths/appearances. The test additionally checks width, containment, inactive application, selected group and unchanged history label.
- Existing warnings remain: EngineDocumentBackendTests non-Sendable capture and the prebuilt blake3 object's newer deployment target; the initial app compile also reports existing Document warnings. No new warning fix was attempted.

Commands, logs, source hashes, screenshots and archive/binding provenance are adjacent. The source-matched export-integration archive was reused; no Rust regeneration. The heavy slot was released after final success and no compiler/test process remained.

## Limits and handoff

This is a focused presentation gate. Full combined Swift validation is assigned to the coordinator's Review/Masks integration candidate; it has not been independently repeated here. Screenshot/OCR verifies painted labels; it does not claim a VoiceOver interaction test. The small production diff preserves every original menu action unchanged. The Loupe overlay slice remains deferred and has no changes in this commit.

The prior ownership evidence under UX-02-ownership and its branch remain preserved. Reused worktree: `/Users/rutmehta/.codex/worktrees/review-ownership/tessera`, new branch `codex/masks-components-clarity`.
