<!-- B5-17b steps 485–490 for apps/mac/ACCEPTANCE.md, section
     "## B5-17. Photo Restoration, painting into channels and alpha display".
     The integrator pastes these between B5-17a (480–484) and B5-17c (491–499). -->

Alpha channel display (B5-17b). Engine backend, a scratch image (never a fixture). The Channels self-test
(`TESSERA_CHANNELS_SELFTEST=<dir>`, see step 176) now also runs 485–488 and prints
`check alpha options: …` and `check reopen <ext> keeps the alpha display (B5-17b)`.

485. **Options are history.** Make a selection, Select ▸ Save Selection as "Sky". Right-click the Sky row ▸ Channel
     Options…: the sheet shows Name, Color Indicates (Masked Areas / Selected Areas / Spot
     Color radio buttons, `document.channels.options.indicates`), the colour well and Opacity 50 %, with Masked
     Areas chosen and the well red. Choose Selected Areas, a green colour and 30 %, OK: History gains exactly one
     "Channel Options" node. Undo restores red / 50 % / Masked Areas; Redo re-applies. OK with only a new name
     records only "Rename Channel"; OK without changes records nothing.
486. **Overlay follows the record.** Turn on Sky's eye with RGB on: the overlay tints the *selected* (white) part of
     Sky green at 30 %. Switch back to Masked Areas: the tint moves to the masked (black) part. Change opacity to
     80 %: the tint deepens. The row's colour chip matches the overlay colour. The RGB composite and a flat PNG
     export are unchanged by any of this.
487. **Saved.** With Sky set to Selected Areas / green / 30 %, File ▸ Save As .tessera-doc, close, reopen: Channel
     Options shows the same indicator, colour and opacity and the overlay looks the same. Repeat with Save As .psd
     (opacity is stored in whole percent). Photoshop opens the PSD with Sky's Channel Options showing Selected
     Areas, the green colour and 30 %.
488. **Photoshop-authored display.** Open a PSD saved by Photoshop with an alpha channel whose Channel Options were
     set to Selected Areas, blue, 70 %: the Tessera sheet shows those values and the overlay uses them. A Photoshop
     PSD with an untouched alpha channel shows red, 50 %, Masked Areas.
489. **Legacy default and kind switch.** A channel from Save Selection or New Channel (and the Quick Mask channel)
     shows red, 50 %, Masked Areas; Quick Mask keeps its own options independent of other channels. Choosing Spot
     Color turns an alpha channel into a spot channel (one node, solidity field); choosing Masked or Selected
     Areas on a spot channel turns it back into an alpha channel with the chosen colour / opacity (one node). Set a
     changed alpha channel back to red / 50 % / Masked Areas: it saves exactly like a never-edited channel.
490. **Validation.** Opacity outside 0…100 % in the field is clamped by the sheet; an engine call with an opacity
     or colour component outside 0…1 or not finite is refused with a status-bar message and no History node
     (covered by `invalid_alpha_display_is_rejected_without_a_node`).
