# Reopen baseline extension — source only, UNRUN

Prepared against main `3a395c69b6946660daf2d30a595149d5ef26e144`. No cache implementation, product behavior, public API, dependency or default change. This agent’s list_artifacts returned an empty agent-local view; root’s inventory showed three attached active worktrees. Historical checkouts had unconfirmed ownership, so preparation used temporary copies. Root subsequently created managed isolated worktree /Users/rutmehta/.codex/worktrees/proxy-reopen-baseline/tessera at 03651eaef1610907b1e495c8f92581d3d6e60c3f and authorized source-only application.

Apply `reopen-baseline.patch` only after source review and coordinator approval. It changes two files: appends one ignored native test and its timestamped listener to existing macOS/test qualification module, and adds a cfg(test) owned EDR allocation helper. The existing process-retained helper is unchanged. The patch has passed read-only `git apply --check`; Rust compilation, formatting and runtime are UNRUN.

The test creates a disposable COPY of the known Sony RAW and builds Compact once. Each process uses exactly one Engine, an initial open and five unchanged closes/reopens. It never edits/saves between cycles. Journal/pixel-container hashes and exact recipe are checked after each close; original fixture and disposable RAW hashes must remain unchanged. Each final callback timestamp begins at public open, includes validation/calibration/setup, and ends at the matching-generation final listener callback; open-return and post-open portions are separately recorded. It is not a physical screen or cold filesystem metric.

Every cycle records actual dimensions/level/settings, selected backend, matching resident receipt, per-frame Metal submission deltas and zero timed GPU pixel readback. Auto is allowed to choose CPU honestly; it cannot pass as GPU. Pixel inspection happens afterward, followed by listener/session/ring/instrumentation release and a bounded five-second Weak Shared/Renderer/GPU operator plus IOSurface release check. Engine shared device lifetime is intentional and is not claimed released by per-session proof. Existing metrics instrumentation cannot reveal calibration-run count; this baseline records timing and route only. Cache hit/miss counters are explicitly deferred until a separate implementation is approved.

`thresholds.json` freezes accepted experimental performance gates and existing numerical gates: SDR absolute <=4/255, EDR <=0.002+0.002*abs(CPU reference), same dimensions/settings/levels only. Neither is a new broad visual-equivalence claim. `run.py` executes four serial processes: auto SDR, CPU SDR, auto EDR, CPU EDR. It preserves before/after native source, fixture, runner and threshold hashes even on process failure; direct exit/log/raw pixels/partial cycle results are retained. Stable baseline CV<=20% is required before future candidate timing. Exclusions are recorded, never silently resampled into a fidelity pass. One pair must be comparable per output format; comparisons of unlike output geometry do not produce speed ratios.

This is only baseline preparation. No candidate paired ABBA execution, warm-edit performance evaluation, cache key hit proof, or benefit claim is supplied by this runner. Future candidate execution must use the approved complete plan and freeze its pairing order before timing. The fresh initial sample per process is not enough to qualify first-open regression; that requires the planned paired experiment.

## Future exact invocation (NOT executed)

After source review, runtime grant, applying the patch and focused compile/strict/fmt qualification:

```sh
python3 /tmp/tessera-preview-reopen-baseline/run.py --execute \
  --checkout APPROVED_CHECKOUT \
  --fixture /Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW \
  --out /Volumes/betterSSD/tessera-validation/smart-preview-reopen-baseline/01
```

Each child uses `cargo test -p tessera-ffi --lib --release engine_same_engine_unchanged_proxy_reopen_baseline -- --ignored --nocapture --test-threads=1`, BetterSSD target `depth-histogram-readonly-77eb68d0-relocated`, deployment15, jobs2. No silent test skip accepted: six completed cycle rows mandatory. Environment preference is explicitly CPU or auto, legacy opt-in is absent.

No workloads were run to prepare these files. The copied plan, thresholds and source manifest travel with the patch. Host paths are local coordinator artifacts, not portable repository dependencies.

Source-only application checkpoint: branch codex/proxy-reopen-baseline on managed worktree above. Independent storage review pending; no compilation, formatting or runtime performed.
