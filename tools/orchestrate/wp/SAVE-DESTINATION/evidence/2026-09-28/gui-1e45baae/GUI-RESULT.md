# Actual Save As GUI acceptance

Completed2026-09-28 using native CUA on owned bundle `dev.tessera.save-as-gui.1e45baae`, PID19398, local profile `/tmp/tessera-save-as-gui-1e45baae/profile`. Exact launch: `open -n "Tessera Save As 1e45baae.app" --args --app-dir /tmp/tessera-save-as-gui-1e45baae/profile`. No folder argument or user document/photo opened. The native folder picker initially displayed its default Documents directory; navigated directly to owned outputs without opening any user file.

Package is copied strict13 Release executable from source1e45baaead70b1bcd208afba800bcacd92213ee8, SHA ce4f532c159381cab26957b9f439f8a6eac823ec0b7481f0a6a461b32937b2ec. It is not claimed byte-identical to full12 relink. Package script/manifest/command logs record unique plist, copied Sparkle, executable-relative rpath and ad-hoc signatures; original executable/framework unchanged. No duplicate build. Script inherited a read-only fixture hash check, but no photograph was copied or opened for this GUI flow.

## Verified real UI flow

1. CmdN created256x192,8-bit,sRGB document. CmdS opened actual Save As sheet; native Choose Folder selected owned local outputs; saved `first.tessera-doc`. Clean title and Saved status observed.
2. Renamed layer through Properties to `Checked Save Edit`. Save As `cancel-sentinel.tessera-doc` produced explicit Replace alert. Clicked Cancel. Title/tab remained first.tessera-doc, Edited; property edit remained. Filesystem proof after-cancel.json verifies BOTH sentinels' exact bytes/hash/inode unchanged.
3. Save As `edited-new.tessera-doc` succeeded with clean title. CmdW closed the document; actual Open Document picker reopened saved output. Properties and Layers showed `Checked Save Edit`, correct256x192 canvas and Opened status. This was a real reopen, not inspection of still-open state.
4. Renamed layer to `Confirmed Replace Edit`. Save As `replace-sentinel.tessera-doc` produced explicit overwrite alert; clicked affirmative Replace. Observed clean replacement title and Saved status. CmdW and actual Open Document reopened replacement; Properties/Layers retained Confirmed Replace Edit, correct256x192 dimensions, Opened status. Screenshot and AX evidence are in CUA conversation; no disk screenshot artifact is claimed.
5. CmdQ ordinary owned-app quit. Exact package-prefix pgrep returned1 (absent); no other app manipulated. final-files-and-exit.json records four output files, canceled sentinel unchanged bytes/inode, replacement changed bytes/inode, original source executable unchanged. No staging leftovers in output directory. Test profile/artifacts retained.

## Observations and limits

Native Open Document row AX and coordinate clicks did not select reliably. A Go To path input attempt had clipboard timeout/partial input; recovered through observed dialog state. Keyboard Down selected known owned row, actual Open button worked. Both reopens completed; no product open/save failure was observed.

After edit/cancel, the bottom HUD retained historical `Saved first.tessera-doc` text while window title and tab correctly showed Edited. The cancel did not produce a new save or alter destination, but the retained status text is a UI clarity limitation and is not used as proof of current clean state.

This validates existing-destination cancellation and affirmative replacement plus new-name save/reopen. No synthetic late GUI race was attempted; late-arrival/no-clobber concurrency remains the separately passed native/Swift barrier coverage. No pixel painting was needed: persisted layer names provide a concrete edit identity. No user photos, existing apps, global settings, protected prompts, or main checkout changed. Runtime/CUA lane released to root after verified exit.
