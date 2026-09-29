# Normalization Task 1 compiled RED

Exact source `bf75fe6664d5b91d7c519816a53e4533b055ce62` compiled successfully (01-compile direct exit 0). The retained exact test executable then ran every required named test: **0 passed, 17 failed, 0 ignored, 70 filtered out**, direct exit 101 (02-red).

The failures are the intended Unsupported scaffold boundary, not compilation/setup failures: positive cases unwrap Unsupported, cancellation/allocation controls receive Unsupported instead of their expected typed errors, and invalid-input controls reject Unsupported as the wrong error type. The frozen source has all three operations return Unsupported. Table-driven tests stop at their first failing row; later rows, phase combinations and subsequent ownership/event assertions have not been exercised. No arithmetic, allocation, cancellation or ownership implementation is qualified.

All four before/after source, runner, oracle and fixture metadata freezes are equal (9140 source entries). The binary before/after identity is equal and its current bytes were independently rehashed. Worktree remains clean.

Executable SHA256: `dc7a46b10ed9abc3ecc911471f49748e916611685c25581c6ac47b23ffc97db3`. See artifact.json, command.json files, direct exit files, full logs, observed.json and RED-OBSERVATION.json. Two expected unused scaffold field warnings are retained in the compilation log.

Only the bounded compile and synthetic 17-test run were executed. No fixture decode, broad regression run, GPU/app workload, source correction or implementation occurred. Legacy linearize remains unchanged. Runtime was explicitly released to root immediately after the bounded outcome; implementation requires a subsequent grant.
