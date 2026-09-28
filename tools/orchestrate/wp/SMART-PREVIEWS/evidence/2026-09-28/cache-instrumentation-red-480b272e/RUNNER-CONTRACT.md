# Task2 RED runner — source only, UNRUN

Runner `run-red.py` pins clean480b272efb3c0d864085442abe531969693d8516 and the approved read-only SonyARW SHA256bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8. Every attempt is a new, nonoverwritten directory. It preserves full source maps, cleanstatus/HEAD, fixture size/hash and runnerhash before/after each child, commands/environment, logs/direct exits, elapsedphase time and interpretedstatus. Compile andbehavior are separate directories. Any compilefailure stops, labeled NOT behavioralRED. No source mutation or implementation is performed by runner.

Environment: remove every inheritedTESSERA_* plus RAW_DECODE_FIXTURES; add only TESSERA_SMART_PREVIEW_RAW explicitSony and empty TESSERA_RENDER_BACKEND(auto). Legacyoptin/profiling/cacheflags absent. Exact CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated, CARGO_BUILD_JOBS=2, MACOSX_DEPLOYMENT_TARGET=15.0. Record buildflags/toolchain/wrapper settings if inherited without recording registry credentials/unrelatedenvironment. Inspector/app/GPU workload must be stopped by coordinator before grant: runner cannot prove exclusive external ownership itself.

Phase1 `cargo test -p tessera-ffi --lib --release proxy_cache_contracts --no-run` must compile. Phase2 uses samefilter with `-- --ignored --nocapture --test-threads=1`. Six ignored groups are explicitly executed. One existing coldvalidationcontrol is expectedpass; five new integrationgroups expectedfail against stubs, potentially after actualRAWbuild/Metalcalibration. No skippedtests, counts-only falsepositive or compilationerror promoted tobehaviorRED. It checks the complete namedfailureinventory andallsixteststarts pluscoldcontrolpass. If a runtimeoutcome differs, preserve and inspect; do not implement based on a misleading parser label.

Outer runner0 means only the preregistered RED was observed, not producttests passed. Directcargo101 remains02-behavior/exit. Expectedcounts are an oracle to be checked, not currentlyobserved evidence. This file andrunner have only undergone text/ASTinspection; no compiler/formatter/test/GPU/app execution.

After independent review and explicit exclusive runtime handoff ONLY:

```sh
python3 /Volumes/betterSSD/tessera-validation/proxy-decision-cache/task2/run-red.py --execute --attempt 01-red
```

Subsequent attempts require newnames (02-red etc). A source revision requires a reviewed runnerrevision/pin and newattempt; never silently overwrite priorattempt/manifests. Fixtures are read-only; testmutations apply disposablecopies. No cacheimplementation/candidatebenchmark authorization follows automatically from runnerstatus.
