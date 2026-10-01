# LR-2 verification evidence

Base: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1`  
RED commit: `3aac4cb67deab186c0ba00afbf780c21549d2dc9`  
Implementation: `20b0fe10d69145493eb9639300775b386c4c784e`

Commands, environment, exclusions, interpretation and limitations are in
[HANDOFF.md](HANDOFF.md). Raw local logs are archived under
`$HOME/.cache/tessera-target/LR-2-tone-curves/lr2-evidence/`.

## Captured test summaries

| Run | Passed | Failed | Ignored | Filtered |
| --- | ---: | ---: | ---: | ---: |
| Initial RED | 2 | 6 | 0 | 0 |
| GPU guard RED | 0 | 1 | 0 | 19 |
| Nonmonotone curve RED | 0 | 1 | 0 | 7 |
| B&W diagnostic RED | 0 | 1 | 0 | 491 |
| Disabled GPU identity RED | 0 | 1 | 0 | 156 |
| Broad synthetic gate | 470 | 0 | 17 | 5 |
| Final focused import and CPU | 11 | 0 | 0 | 0 |
| Final focused GPU guards | 2 | 0 | 0 | 155 |

Counts for the focused reruns overlap the broad gate; do not add them as unique
tests. All final runs exited zero. RED runs intentionally exited 101.
The broad gate excludes five real-RAW cases, with existing benchmarks ignored.

## Representative observed RED failures

- Curve midpoint absent: normalized x expected `128/255`, but no curve populated.
- B&W enabled field absent: JSON `Null`, expected `true`.
- Legacy controls: `(0,0,0,0,0)`, expected `(1.5,25,30,-20,-10)`.
- B&W red swatch: `[1,0,0]`, expected three channels at `0.2627`.
- All mixer bands +50: red output `1`, expected `0.39405`.
- Nonmonotone curve incorrectly populated a curve that CPU spline validation rejects.
- B&W approximation diagnostic absent.
- Enabled B&W GPU parameter creation returned success instead of rejection.
- Disabled B&W changed the GPU identity parameter from 0 to 1.
- Synthetic SQLite end-to-end initially failed because translated keys remained
  in `lrcat_develop_source`.

The first GPU-test attempt had an unavailable test-only serde_json import. It was
replaced by a typed setting without adding a dependency, then failed at runtime
before the guard was implemented. Probe tooling initially selected incompatible
cached rlibs after multiple feature builds; it now uses Cargo JSON artifact paths
and both probes were rerun successfully.

## Final measured probe output

```text
synthetic import: 4 swatches / 12 channels, maximum absolute error=0.000000000, tolerance=0.000002
synthetic extended curves: 4 catalog imports / 12 channels, maximum absolute error=0.000000030, tolerance=0.000002
29c byte compatibility: 5 synthetic recipes, 56666 serialized bytes identical to 87ff1ff1
```

The B&W and curve probes compare against declared analytic reference formulas,
not Adobe-rendered goldens. The importer comparison compiles the lane-base
importer against current shared engine dependencies; engine-api's existing
fingerprint tests also pass.

## Other gates

- `cargo clippy --locked -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all --check`: exit 0.
- `git diff --check`: exit 0.
- No Cargo.lock or Cargo.toml dependency changes.
- No Swift gate or app launch.
