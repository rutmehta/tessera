# B5-13 handoff: Liquify workspace and Content-Aware Move / Extend

Branch `wp/B5-13`, base `c15dee24` (main). Machine A reviewed `4de8b0b7`; the commits after it fix that review's
blockers. Not rebased: A asked for a rebase only after B5-16 / B5-17b land. Expected conflicts are B5-16 and B5-18 in
`DocumentView.swift` / `SmartFilterRows.swift` and B5-19 in the `document.rs` module block, all "keep both sides".

## Commits

| Commit | What |
| --- | --- |
| `ea787a7a` | Brief |
| `8319056b` | Engine: `document/liquify.rs`, `document/content_aware.rs`, narrow B5-13 helpers in `filters.rs`, tests |
| `33453c3c` | App, bindings, Swift tests. A noted that this commit does not compile on its own (it needs `e22ec3dd`/`ef62022e`). It is left as is because rewriting history is not allowed before the rebase. Squash it at merge if bisectability matters. |
| `e22ec3dd` | Liquify stroke-end ordering, canvas clipping, wider panel |
| `ef62022e` | Adopts main's RequestCancellation, regenerated bindings, ACCEPTANCE 400–419 |
| `4de8b0b7` | Self-test opens its document, evidence, READY entry (A's review target) |
| `6838ca64` | RED: deterministic cancel-at-checkpoint engine tests |
| `817f854d` | Fix: a cancelled apply never lands in history; the CAM preview cancel is checked where it is shown |
| `7387d11d` | RED: Swift, a cancelled Liquify apply that the engine committed late must be undone |
| `84dbe787` | Fix: Swift `.discarded` handlers undo a job that succeeded (B5-09 parity) |
| (this commit) | HANDOFF.md, ACCEPTANCE 409 note |

## Review fixes (A's review of 4de8b0b7)

1. **A cancelled Apply never lands in history.**
   - Smart objects: before the fix, the smart-filter path (`set_adapter_smart_filter` → `set_nodes_checked`) ran its validation render under the document lock with a fresh flag that nothing ever set. It now takes the job's own flag (Liquify `cancel`, Content-Aware Move `closed`) through `set_nodes_checked_until`. That flag stops the validation render.
   - Pixel layers: `edit_layer_checked` now goes through `edit_checked_until`. On both paths the flag is checked again under the document lock, immediately before `st.doc.apply`, so the outcome is decided at the write.
   - Residual window: a cancel that arrives after that check finds the node already written, and the call returns success. Item 2 covers that case.
   - `set_nodes_checked` and `edit_checked` keep their signatures and wrap the new functions with a fresh flag, so other callers are unchanged.
2. **Swift `.discarded` handlers.**
   - `DocumentLiquify.apply` and `DocumentContentAware.apply` now undo a discarded job that succeeded (`doc.run("Undo cancelled …") { undo() }`), mirroring B5-09 `DocumentRetouch`.
   - The Content-Aware completion moved into `applyEnded(_:label:doc:)` so it can be tested directly.
3. **Deterministic cancel tests.**
   - `DocumentSession::set_apply_checkpoint_hook` is test support, `#[doc(hidden)]` and not exported over UniFFI. It is scoped per document, so parallel tests do not interfere.
   - It fires at three sites: `liquify:render` before the full-resolution render, `content-aware:show` before a preview is shown, and `write` under the document lock just before the history node.
   - The tests cancel at exactly those points, for pixel layers (all three Liquify destinations), smart objects (a new Liquify filter and a re-edit), and Content-Aware Move on pixel layers and smart objects. Each has a no-cancel control that reaches the same checkpoint and commits one node.
   - The old test that accepted either outcome is replaced by one that cancels at the render checkpoint.
4. **Content-Aware Move preview (should-fix).** `show_layer_preview` checks the preview's cancel flag under the preview-slot lock. A cancel sets the flag and then clears the slot under that same lock, so the cancelled preview can no longer be installed after the cancel's `clear_preview`. Test: `a_cancel_just_before_the_preview_shows_never_displays_it`.

Files outside the WP's own modules: the `filters.rs` edits are marked `// B5-13` or sit in the B5-13 block. `edit_checked` and `set_nodes_checked` are now wrappers over the `_until` variants: one checkpoint line and one cancel check each, and the validation render takes `cancel`.

## Gates (final head)

All gates ran with `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-13`.

- `cargo test -p tessera-ffi --no-fail-fast`: 447 passed, 3 failed. The machine was at load average 33–45 from parallel agents. Each failure passed when rerun alone:
  - `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`: p95 171.6 ms against the 250 ms limit alone.
  - `develop::export_batch_does_not_starve_slider_drag`: a timing test.
  - `smart_preview_thumbnail::hdr_saved_offline_recipe_…`: a "close the active Smart Preview editor" conflict.

  An earlier fail-fast run failed only `document_viewport::save_render_undo_races…` (`saves > 0`), which also passes alone. None of these tests touch B5-13 code.
- The new cancel tests are deterministic and pass: `document_liquify_ui` 15 tests and `document_content_aware_ui` 14 tests.
- `cargo clippy -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- `apps/mac/build-ffi.sh`: OK. Bindings are unchanged, since no UniFFI surface changed.
- `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK, 724 tests, 3 skipped, 0 failures.

## Remaining on-screen checks (Machine A)

- 409 and 416 on screen: Cancel during a long Apply returns at once and History is unchanged. The undo of a job the engine committed late is covered by unit tests only.
- Brush feel on a real tablet (pressure) and freeze/mask overlay tracking while zooming and panning (ACCEPTANCE 404/405).
- Content-Aware Move: dragging the ghost at several zoom levels, and the preview busy/Cancel state (411–415).
- Inspector at 1440 pt (419).

## Performance notes from A (not required now)

- Opening Liquify on a 20 MP layer makes a full-resolution f32 RGBA copy of the source (~320 MB) in `begin_liquify` (`read_rgba` plus the `Raster` snapshot). A follow-up could keep the tiled raster and build the proxy from tiles.
- Cross-document stall: the Liquify job registry is one global `Mutex<HashMap>`. `jobs()` locks every job while pruning, so a long operation holding one document's job lock (a brush batch or a preview) stalls Liquify calls on other documents. A follow-up could make the registry per document or skip the lock during pruning.
