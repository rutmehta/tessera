<!-- B5-17c steps 491–499 for apps/mac/ACCEPTANCE.md, section
     "## B5-17. Photo Restoration, painting into channels and alpha display".
     The integrator pastes these after B5-17b (485–490). -->

Painting into channels and Quick Mask (B5-17c). Engine backend, a scratch document (never a fixture). The
self-test `--channel-paint-selftest=<dir>` (launch with `--new-document`; add `--channel-paint-selftest-hold <s>`
to linger per step) runs 491–499 on its own 1200 × 800 document through the same models the UI drives, printing
`channel-paint-selftest: step <n>-<name> window-id <id>` and `check <name> ok|FAIL`, ending with
`done, <n> failure(s)`.

What a stroke paints, in order: Quick Mask on → the Quick Mask channel; else an alpha or spot channel that was
targeted by a plain click on its row in the Channels panel (and is still highlighted) → that channel; else the
primary layer's pixels, or its mask (Paint Mask on and the layer has one, or any non-pixel layer). A plain click
on RGB or a colour row goes back to the layer. ⌘-click (load as selection) and rows highlighted by Save
Selection / New Channel do not target. The status bar says "Painting into Quick Mask" / "Painting into <name>".

491. **Quick Mask.** With no selection, press Q (the Quick Mask row appears, "Temporary"). Brush, black
     foreground, paint a band across the image: the red overlay covers the band. Press Q again: the selection is
     everything except the band (marching ants around the image edge and around the band). Q, paint white over
     part of the band, Q: that part is selected again. History: "Quick Mask", "Brush Tool", …, "Quick Mask".
492. **Paint into a saved alpha channel.** Channels ▸ + ▸ New Channel ("Alpha 1", black). Turn its eye on and
     click its row. Brush, white foreground, paint a stroke: the overlay changes where you painted while you drag,
     and the row thumbnail shows the white stroke. One "Brush Tool" row per stroke. ⌘-click Alpha 1: the
     selection is the stroke's shape.
493. **Eraser on a channel.** With Alpha 1 still targeted, the Eraser paints the channel toward black (masked)
     wherever it passes, regardless of the foreground colour; its History row is "Eraser". Grey foreground with
     the Brush paints grey (partial selection) — the channel takes the colour's luminance.
494. **Undo per stroke.** ⌘Z after each channel stroke removes exactly that stroke from the channel (thumbnail and
     overlay follow); ⇧⌘Z puts it back.
495. **Selection clips channel paint.** Make a rectangular selection, paint across its edge into Alpha 1: only the
     part inside the selection reaches the channel. (Quick Mask entered *with* a selection: see the note below.)
496. **Layer painting is unchanged.** Click RGB in the Channels panel: the Brush paints the layer's pixels again.
     On an adjustment layer (or a pixel layer with Paint Mask on), the first stroke still creates a reveal-all mask
     and paints it. A locked layer does not block painting into a targeted channel, and a group selected in
     Layers does not either.
497. **Channel thumbnail updates live.** While a stroke into a visible channel is in progress, its row thumbnail
     and the overlay refresh several times a second (throttled to ~150 ms), not only on mouse-up.
498. **Saved.** Save As .tessera-doc, close, reopen: Alpha 1 has the painted strokes (⌘-click gives the same
     selection) and can be painted again. Repeat with Save As .psd (channels are stored at 8 bits).
499. **Composite never changes.** Channel and Quick Mask strokes leave the RGB composite, the layer's pixels and
     its mask exactly as they were (a flat PNG export before and after is byte-identical). The Clone Stamp and
     Healing Brush refuse to paint into a channel with a status-bar message and no History row; a stroke into a
     channel deleted meanwhile fails the same way.

Note (open issue for the Channels owner): Quick Mask entered while a selection is active keeps that selection
active in the engine, so Quick Mask strokes are clipped to it (white cannot extend the selection outside it until
you Deselect). Photoshop drops the marching ants on entering Quick Mask. Fix belongs in `QuickMask.enter` /
`DocumentChannels.toggleQuickMask` (deselect as part of entering), not in the stroke path.
