# Private RAW capture Task3 checkpoint

Candidateaf5473d8e34f4a62201fbadedad6fa8b64aed20b, codex/private-raw-capture. BaseTask2d9d805cb. Clean committed source, unmerged pending independent whole-branch review.

01-consumer-red exit101:6newbehaviorfail,1existingmismatchcompositioncontrolpass against Unsupported test seam. No compile fixes.
02-consumer-green exit0:7held-consumer tests pass.
03-full-raw-decode exit0:57unit+3integration pass,0ignored,0doctests. Includes all42capture tests (12ownership+23stream+7consumer) and existing5RAWfixture decode control.
04-strict exit0:Release all-target Clippy -Dwarnings.
05-fmt exit0:workspace fmt check.
git diff --check exit0. Existing vendorLibRaw C++ warnings retained; no warning-free native compiler claim.

Exact commands/direct exits/fulltracked+newfile manifests retained; each run before/after identical, finalmap matches commit. Explicit sharedBetterSSD target/MACOSX15/jobs2. Fiveexistingfixture hashes captured before thislane and unchanged afterallgates. New tests synthetic only.

Task3 adds a private cfg(test,unix) callback seam, seven tests and ownership documentation. No production capture behavior change or public path/callback/decoder API. Caller mismatch composition is an existing gate control, not new productiondecoder admission. Originalreplacement consumes heldA; delayedreader retainsstage/quota; panicRAII releases; cancellationrefusescallback; primaryconsumererror survivessecondarycleanup; successrequirescleanup success.

CapturedRaw means verified ephemeral bytes, not owned decoded pixels or immutable recipe. Mixedstream/atomic snapshot distinction remains explicit. Future closed decoder adapter, recipe/catalog resolver, persistent store, render/dependency fidelity and BUI remain out of scope. NonUnix captureUnsupported; no cross-platform qualification. Compiler lane released after allcommands finished; independentwholebranchreview pending.
