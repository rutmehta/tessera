# Phase 10 commands (planned; UNRUN)

All probe builds and run data belong under `/Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/`. Do not overwrite an existing phase directory.

Compile from this probe directory, writing the binary to the external validation volume:

```sh
xcrun --sdk macosx clang -O2 -fobjc-arc -framework Foundation -framework ImageIO -framework CoreGraphics imageio-probe-context.m -o /Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/imageio-probe-context
```

Run once with an empty unique output directory:

```sh
python3 run_context_headroom.py --binary /Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/imageio-probe-context --run-root /Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/context-headroom-phase10/run-01
```

The runner starts separate default, target8, and target16 processes for each input. It clears inherited decode/target/output variables and sets only the per-run output locations plus the requested target for non-default cases. Each process has a 60-second timeout. Preserve a failed attempt under its own run directory and do not reuse that path.
