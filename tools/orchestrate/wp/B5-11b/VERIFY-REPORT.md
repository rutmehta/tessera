# B5-11 on-screen verification (Machine B verifier, 2026-09-27)
Build wp/B5-11 @ b923b89, release, `open -g -n`, scratch --app-dir, 1200×800 scratch PNG, window 1440×900. Driver: Codex background CU (no focus/cursor change observed). Modifier-held drags and Esc-during-drag cannot be sent (BLOCKED).
No step FAILS. Passes: 360, 361, 362 (⇧⌥ blocked), 363 (⇧ blocked), 364, 365, 366, 367, 368, 369 (⌥ blocked), 370 (+ hole hit test), 372 (modifiers blocked), 373, 374, 375, 376, 377, 379. 371 partial (100 % only). 378 not run.
Findings:
- 366: enabling Stroke on an existing shape takes the FILL colour, so dashes are invisible until recoloured. Dash-offset slider keyboard steps produced 3 "Edit Shape" rows in one adjustment.
- 367: the previous anchor's direction handles are not visible in the Pen draft after the next point is placed.
- 368 BUG: pressing A twice stays on Path Selection (spec: A twice or ⇧A → Direct Selection); ⇧A works.
- 373: Layers row shows the raster mask thumbnail but no separate vector-mask thumbnail.
- 375: Edit ▸ Undo via the menu stalled once under CU (⌘Z worked).
- 376: on a pixel-locked shape, a rejected recolour leaves the colour well showing the rejected colour until the layer is reselected.
- 379 CHECK: after choosing Drop Shadow and Stroke from the FX menu, Outer Glow and Bevel & Emboss were also present (possibly CU misclicks; unconfirmed). Properties is short at 900 pt, so Shape/Fill/Stroke need scrolling.
- Tool shortcuts (U, ⇧U, Z) are ignored while a Properties slider or the Layers list has keyboard focus; Esc or a canvas click is needed first.
- Path Selection: clicking empty canvas does not deselect.
- Stale status hints: "Pen path discarded" after Return created a shape; the Remove hint persists after switching to Move.
- Properties shows Position 0,0 / Bounds "Whole canvas" for newly drawn shapes regardless of placement.
- Known, not re-reported: the toolbar overlaps the options bar (Machine A's M2-56).
Screenshots (verifier worktree): .worktrees/verify/tools/orchestrate/wp/B5-11/evidence/v-*.png
