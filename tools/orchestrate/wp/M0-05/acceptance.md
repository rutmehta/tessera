# M0-05 — computer-use smoke test of the app shell
Repo root: /Users/rutmehta/Developer/tessera. Evidence dir: /Users/rutmehta/Developer/tessera/tools/orchestrate/wp/M0-05/evidence (create it).
1. In a terminal: `cd /Users/rutmehta/Developer/tessera/apps/mac && bash Support/make-app.sh` — expect it to finish without error and produce build/PhotoEditor.app (or Tessera.app if renamed; use whichever exists).
2. Launch it: `open build/PhotoEditor.app --args --folder /Users/rutmehta/Developer/tessera/fixtures/raw --front` (adjust app name if needed). Wait 5 s. Expect a window with a grid of 5 thumbnails (raw files: CR3, ARW, NEF, RAF, DNG). Screenshot with `screencapture -x <evidence>/step-2.png`.
3. Click the first thumbnail, press X. Expect a reject badge on it. Screenshot step-3.png.
4. Press P on the next image (it should have auto-advanced or use → then P). Expect a keep badge. Screenshot step-4.png.
5. Press ⌘Z. Expect the last decision to revert. Screenshot step-5.png.
6. Quit the app with ⌘Q.
Report each step pass/fail with a one-line note.
