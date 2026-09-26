# M2-29v — Develop a rendered JPEG in the loupe (on-screen)

Setup (never edit source files; capture only the Tessera window with the computer_use screenshot tool):
1. Build: `export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-29v; (cd apps/mac && ./build-ffi.sh && swift build && Support/make-app.sh release)`; the last line reads `Built …/apps/mac/build/Tessera.app`.
2. Scratch: `SCR="$(mktemp -d)"; swift apps/mac/Support/make-sample-folder.swift "$SCR/shoot" 12; defaults delete dev.tessera.app 2>/dev/null; true`. Also copy a real photo JPEG if one exists under fixtures/ (`find fixtures -iname '*.jpg' -size +200k | head -3`) into `$SCR/shoot`.
3. Launch: `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/shoot"`; wait 5 s. 📸
Read the "Identifiers" appendix sections of apps/mac/ACCEPTANCE.md for control identifiers; prefer keyboard-first interaction.

Steps (report pass/fail with exactly what you saw for each):
1. Click the first JPEG cell, press **Return** to enter the loupe, then **D** to open Develop. 📸 Expect a non-black rendering of the JPEG in the loupe (not a "RAW only" error), the Basic panel with sliders, and the histogram populated (it must NOT say "Open a RAW in the loupe").
2. Drag **Exposure** to about +1.0 (or focus the slider via its identifier and press the right arrow ~10 times). 📸 Expect the image to brighten during the drag and a refined frame to settle within a second.
3. Move **Contrast** and **Tint** similarly. 📸 Expect visible changes.
4. Press **Reset** (or the panel's Reset control). 📸 Expect the original rendering.
5. Set Exposure to +1.0 again, then quit with ⌘Q and relaunch with the same command. Open the same JPEG in Develop. 📸 Expect the edit persisted (image still brighter, Exposure shows +1.0). Then run `grep -l '"source_kind": *"rgb"' "$SCR/shoot"/*.json "$SCR/shoot"/.tessera*/* 2>/dev/null; find "$SCR" -name '*.json' | xargs grep -l source_kind` and report which file contains `"source_kind": "rgb"`.
6. Export: ⇧⌘E, choose preset **Full-size JPEG**, destination `$SCR/out`, Export. Then `sips -g pixelWidth -g pixelHeight "$SCR/out"/*.jpg` and open the exported file with Preview (`open "$SCR/out"/*.jpg`); 📸 capture the Preview window. Expect the same orientation and the brighter edit visible.
7. Return to the grid (⌘1) and repeat step 1 on the real photo JPEG if one was copied. 📸
8. Quit with ⌘Q.
