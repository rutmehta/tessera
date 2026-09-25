# M0-05 — computer-use smoke test of the Tessera app (real engine)
Repo root: /Users/rutmehta/Developer/tessera. Use absolute paths everywhere. Take a screenshot with the computer_use screenshot tool after every step.
1. In a terminal: `cd /Users/rutmehta/Developer/tessera/apps/mac && ./build-ffi.sh && bash Support/make-app.sh` — expect success and `build/Tessera.app`. If anything fails, paste the last 20 lines of output in your note and stop.
2. Launch: `open /Users/rutmehta/Developer/tessera/apps/mac/build/Tessera.app --args --folder /Users/rutmehta/Developer/tessera/fixtures/raw --front`. Wait 8 s. Expect a window with a grid of 5 thumbnails. If the status bar shows an error, copy its exact text into your note.
3. Click the first thumbnail, press X. Expect a reject badge on it.
4. Press → then P. Expect a keep badge on the second image.
5. Press ⌘Q. Then relaunch with the same command as step 2 and wait 8 s. Expect the reject and keep badges to still be present (decisions persisted to sidecars). Also run `ls /Users/rutmehta/Developer/tessera/fixtures/raw/` in the terminal and report whether .xmp sidecars exist next to the raws.
6. Press ⌘Q.
Report each step pass/fail with a one-line note.
