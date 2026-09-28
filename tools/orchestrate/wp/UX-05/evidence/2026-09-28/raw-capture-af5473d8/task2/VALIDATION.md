# Private RAW capture Task2 checkpoint

Candidate d9d805cb6586f39cf55bcd2a0bad7f76451a3708, codex/private-raw-capture. Base Task1 01409e82; source-only reviewed checkpoint03f5b42d. Only rustfmt changes since source review. Branch unmerged; Task3 pending.

01-stream-red exit101:23compiled tests fail against Unsupported before implementation,0pass. Preserved; no missing-symbol compilation or test repair needed.
02-stream-green exit0:23streamtests pass.
03-full-raw-decode exit0:50unit+3integration pass,0ignored,0doctests. Includes12Task1 ownership regressions and existing5RAWfixture decode test.
04-strict exit0: Release all-target Clippy -Dwarnings.
05-fmt exit0: workspace fmt check.
git diff --check exit0. Existing vendorLibRaw C++ warnings preserved, not claimed warning-free.

All commands use explicit shared BetterSSD target/MACOSX_DEPLOYMENT_TARGET15/jobs2. All five source manifests unchanged within each run; final file map matches committed files. Five existingRAWfixture hashes captured BEFORE this GREEN lane and unchanged afterward. New primitive tests use only synthetic bytes. No source fixture writes, decoder behavior/UI/GPU/archive changes.

Bounded copy/read-only sealed stage/re-read digest/expectedidentity/cancellation/source-metadata behavior qualified on macOS. Metadata detection is best-effort; identity names captured stream, not atomic original snapshot. Public owner has no path or writable descriptor escape. NonUnix capture returns Unsupported; no nonUnix build qualification. No decoder consumer or render admission is added. Independent final Task2 source/evidence review pending. Compiler lane released after all commands finished.
