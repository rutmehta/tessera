# Phase 10 commands and provenance

The measurement source checkpoint was `b501cc96b50562f4b738465b27db12427cf6446b`. Probe and runner source hashes remained unchanged through attempt03. All native binaries and run data were stored under the BetterSSD validation root shown in the preserved command/freeze files under `evidence/attempts/`.

The probe was compiled once for attempt01 with the following command; compiler stdout, stderr, and direct exit are preserved there. The same binary was reused for attempt03, with SHA-256 `5a0ca1f883de5aaabca91691a2666ad016d80fff150b20660ded7d0932b580c0` and mode `0755`.

```sh
xcrun --sdk macosx clang -O2 -fobjc-arc -framework Foundation -framework ImageIO -framework CoreGraphics tools/orchestrate/wp/EXP-45/evidence/2026-09-28/context-headroom-phase10/probe/imageio-probe-context.m -o /Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/attempt-01/imageio-probe-context
```

Attempt01 ran the default A80 case and stopped at the first runner's overly strict whole-ICC hash check. Attempt02 did not launch because a copied binary lost its executable mode. Both attempts are preserved and labeled. Attempt03 used the verified executable copy and this command, with a fresh output root:

```sh
python3 tools/orchestrate/wp/EXP-45/evidence/2026-09-28/context-headroom-phase10/probe/run_context_headroom.py --binary /Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/attempt-03/imageio-probe-context --run-root /Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/attempt-03/runs
```

The runner runs one file in each process, clears inherited decode/target/output variables, then performs default/no setter, target8, and target16. It saves the command/environment, per-run pre/post hashes, stdout, stderr, warning text, and direct exit. It requires phase8 default parity before each file's nonzero-target cases. Attempt03's exact argv and external paths are also preserved in its per-run `command.json` records.
