# GUI handoff: PSD copy and explicit stub diagnostics

**Hold:** source-only plan. Do not build or launch from this checkout. Wait for the combined PSD + stub-UI candidate's full current-FFI suite and the root's desktop grant. The existing normal preview PID 57591 belongs to the user; do not quit, relaunch, or interact with it.

## Combined candidate package

Use only the exact Swift executable from the passing combined full suite at `workspace-redesign/codex/psd-current-ffi` (root reported source HEAD `729962d9`). Package it as a new unique validation app name and bundle ID, with Sparkle/rpath/signing done as in the prior `package-hashes.txt` evidence. Use a fresh `/tmp` support path, no document-handler registration, and do not touch the already-open preview. Launch only after the root grants the desktop slot. Keep the PSD/stub candidate identity and package/executable hashes in the run record.

The GUI input is the committed generated 320×240 checkerboard JPEG (`fixtures/jpeg-output/Fixture long name - display proof test.jpg`) with its neutral XMP sidecar. The setup script below copies it to a new disposable catalog and creates separate support/output paths; it does not launch an app or modify the committed input. Do not use the Sony RAW for this PSD check.

## Ordered checks

1. Start the combined validation app without a folder argument and with fresh disposable support. Confirm Library says `No folder open` / `No images`; the empty-state view has `Open Folder…` and does not show `Load 20,000 Stub Items`. Inspect the `Debug` menu and confirm it has no `Load 20,000 Stub Items` command. Record the menu items visible, not just a source inference.
2. Quit that disposable instance. Relaunch the same validation package with the explicit UI-diagnostic launch flag `--enable-stub-library` and a different fresh support path. Confirm the empty-state action and Debug menu command are present. **Do not activate either one.** Quit. This verifies visibility only and must not create synthetic records.
3. Relaunch the same package against the prepared disposable JPEG folder and its separate support path (only `--folder <fixture>` and `--app-dir <temp-support>`). Before edits, record source JPEG and XMP SHA-256.
4. Select the checkerboard image. Use `Library ▸ Open in Layers…` (or ⌘E), inspect the confirmation sheet, then choose `Open in Layers`. Confirm the layered document opens and its content is visibly the selected 320×240 photo.
5. Use `File ▸ Save Rasterized PSD Copy…`. In the native save panel save to the prepared `psd-output` folder using the suggested rasterized-copy name. Do not add a filter, transform, large image, or delay merely to force a heavier path. Confirm the completion status and that a nonempty `.psd` exists.
6. Recompute source JPEG and XMP hashes; they must exactly match the pre-edit values. Record the PSD path, byte size, file type, and dimensions if the platform tool reports them. Use `File ▸ Open Document…` to reopen that saved PSD, and confirm a document opens with the image/layer content visible. Do not claim pixel parity from visual inspection.
7. Quit only the disposable validation process. Preserve the prepared catalog, support DB, PSD, logs, and hashes for review. No artificial cancellation run: deterministic PSD/native tests in the combined gate cover cancellation/close handling, and this tiny UI flow should remain small and responsive without making a timing claim.

## Preparation script

`prepare-psd-stub-ui-gui.sh <repo-root> [new-empty-temp-root]` refuses an existing destination. It copies only the small generated JPEG and neutral XMP into a temporary input folder, creates empty PSD-output and isolated normal/diagnostic support folders, and writes an input hash manifest. It does not launch Tessera or build anything.

## Deferred evidence

Record no-stub controls hidden in ordinary mode and visible under explicit diagnostic opt-in, successful actual PSD file save/reopen, unchanged input hashes, exact candidate/app/support paths, and any failures. Keep prior BLAKE3 deployment-target warning and macOS 15 compatibility limit attached to the app provenance. No broad compatibility, throughput, PSD feature-completeness, or pixel-identical claims.
