# Controlled P10 pixel-parity diagnostic

Executed after the parent granted the serialized heavy slot. The first attempt
failed to compile because the harness incorrectly called `is_empty()` on the
unit result of `set_settings`. After removing that diagnostic-only assertion,
both builds and all four captures passed; both A/B pairs were byte-identical.
`parity-capture/` preserves the failed attempt, and `parity-capture-retry/` records
the completed comparison. `parity-plan.json` retains the original preparation
snapshot; the retry report records the corrected harness hash. This diagnostic
does not change production source or establish presentation latency acceptance.

The original command was:

```sh
python3 /Users/rutmehta/Developer/tessera/.worktrees/M2-58/tools/orchestrate/wp/M2-58/run_pixel_parity.py --run
```

The corrected attempt added `--output tools/orchestrate/wp/M2-58/parity-capture-retry`
from the M2-58 checkout. Further executions require the parent's serialized heavy
slot. Without `--run`, the command only verifies pinned tracked Rust source and prints
the plan. `parity-plan.json` records that preparation. The runner rejects an
existing output directory; use `--output /absolute/new/path` for a later attempt.

The identical `parity_capture.rs` is temporarily installed under a unique Cargo
integration-test name in the existing main and M2-58 checkouts. The runner checks
their product source against `c0d4535` and `5ff2679`, respectively, records observed
HEADs and resolved pins, and refuses tracked source differences. It never
overwrites an existing temporary-test path. Cleanup removes only its own file
with the original harness SHA-256; changes by another writer are retained and
reported. No new worktree is needed.

Each existing release target requires a new integration-test binary to compile
and link. Cached dependencies should be reusable, but Cargo may recompile the FFI
library or dependencies if feature/configuration fingerprints differ. There is
no Swift build or binding generation. Both builds use
`MACOSX_DEPLOYMENT_TARGET=15.0` and separate existing external target directories.

Four fresh processes capture before/after pairs at exposure 0 and +1 with Auto
Upright, native process revision 2, requested GPU backend, a 1280×900 viewport and
RGBA8 output. Each creates a fresh engine and copies only the same Sony ARW into
a fresh folder, excluding prior sidecars. The initial frame settles before the
settings update; the newer final frame is copied synchronously during its
callback while producer serialization still protects it. Every valid pixel row
is copied; IOSurface padding is excluded. This is cold application/session/cache
state, not a purged OS or GPU-driver cache.

Pass requires exact equality of valid pixel bytes, dimensions, final level,
reported backend, normalized settings, and full RGB/luminance histograms. The
runner records fixture/harness/executable/source hashes, exit codes, elapsed
times and load averages. Timings are diagnostic durations, not presentation
measurements. No tolerance or changed-byte allowance is applied.

Baseline `FrameInfo` cannot report actual residency. Metadata explicitly leaves
that value null for both sides: selecting GPU and reporting Metal do not prove a
particular frame's route. Exact output equality remains useful independently.
This small fixture check can establish only its controlled settled-output
parity; it cannot establish all-image parity, P01 presentation latency, or P11's
200 ms / 10% criteria.
