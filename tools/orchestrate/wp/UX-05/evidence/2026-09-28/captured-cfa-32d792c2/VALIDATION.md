# Closed captured-CFA decoder qualification

Candidate 32d792c25d65e4f1c551f8abab360b52432b3d3d, branch codex/captured-cfa-decoder; base main3a395c69b6946660daf2d30a595149d5ef26e144. Clean committed source; unmerged pending independent final review.

01-red exit101:10behaviorfail,1already-passing Unsupported refusal control,2explicitignored qualification tests; compilation succeeded. Failure preserved.
02-green exit0:11ordinary tests pass,2qualification tests ignored explicitly.
03-five-family exit0:both previously ignored qualification tests executed,2pass/0ignored. Positive named inventory: SonyARW4928x3276(16,144,128owned samples); FujiRAF4992x3296(16,453,632); NikonNEF7424x4924(36,555,776); CanonCR36288x4056(25,504,128); CFA DNG5216x3472(18,109,952). Exactu16/fullmetadata equality after replacing originalCOPY with invalidB and deleting stage/pool, plus Sony cancellation after nativeopen/afterdecode success. Fake decoder-drop tests are protocol evidence only; actual RawSource scope/Drop source proves native-close ordering, not instrumented libraw_close telemetry.
04-full exit0:68unitpass,2ignored(actualqualification separately executed03),3integrationpass,0doctests. Total71 ordinary full-suite passes, not73 ordinary cases.
05-strict exit0:Release all-target Clippy -Dwarnings.
06-fmt exit0:workspace fmt check.
git diff --check exit0. NativevendorLibRaw warnings remain in logs; not a warning-free native compiler claim.

All six runs have exact unchanged inputmaps and unchanged5fixturehashes before/after. Harness requires allfive named familyfiles and prior expectedhash/size before invoking commands. Sourcefixture originals read-only; tests mutate isolated copies only. Explicit sharedBetterSSD target/MACOSX15/jobs2 used everycargo command. Final source map matches commit.

Closed public output: owned CfaU16+RawMetadata+capturedidentity+route. No pathname/reader/RawSource/nativehandle/lazy/generic public output. Preserves capturedstream vs atomic snapshot distinction. Rejects LinearRaw route; no RGB/render/recipe/catalog/FFI integration. Cancellation between native calls only; no decompressed/nativeRSS bound; nonUnix unsupported/unqualified. No native archive or bindings regenerated. Runtime lane released after allcommands exited.
