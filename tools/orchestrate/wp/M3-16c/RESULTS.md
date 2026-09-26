# M3-16c results

## Restored prerequisites

- Stable prerequisites: `crates/ml-enhance/training/requirements.txt` and
  `crates/ml-enhance/training/test_training.py`.
- Created the local ignored venv at `crates/ml-enhance/training/.venv` using
  Python 3.13.3 on macOS 26.6.2 arm64. `pip check` passed.
- Installed torch 2.8.0, numpy 2.2.6, onnx 1.18.0, onnxruntime 1.22.1,
  onnxconverter-common 1.16.0. PyTorch reports MPS built and available; smoke
  training deliberately uses CPU. An initial converter 1.14.0 pin conflicted
  with ONNX's protobuf requirement; the tested final pin is 1.16.0.
- Driver uses unittest (no pytest dependency), checks noise calibration, runs
  300 synthetic training steps, and exports both ONNX variants under a shared
  180-second budget with subprocess timeouts. Rust retains all quality, seam,
  and executed-provider checks. No real RAW files or pretrained weights used.

## Measured full integration

Executed with `CI` unset and `TESSERA_TRAIN_PYTHON` pointing to the installed
venv, including a successful run with a repository-relative interpreter path:

```sh
cargo test -p ml-enhance --release --test cfa_model -- --nocapture
```

| Measurement | Observed result |
| --- | --- |
| Calibration | Known per-plane shot/read coefficients recovered within 10% |
| Held-out samples | 24 crops, four planes, 32×32 packed pixels |
| Input PSNR | 26.4397791854 dB |
| PyTorch output PSNR | 32.4784356225 dB |
| ONNX fp32 PSNR gain | 6.0386564641 dB |
| ONNX fp16 PSNR gain | 6.0386721805 dB |
| fp32/fp16 maximum tiled vs whole error | 0 / 0 |
| First-run training time (trainer report) | 2.286096 s |
| First-run calibration + training + export | 11.194 s |
| First-run full Rust integration, excluding compilation | 12.193 s |
| Final explicit run training time | 2.175296 s |
| Final explicit run calibration + training + export | 4.473 s |
| Final explicit run full Rust integration | 5.825 s |

Rust loaded newly exported fp32/fp16 ONNX files through ModelRegistry and
CfaDenoiser. Reports contain CoreML partitions and CPU Resize/Slice nodes.
The existing legacy TorchScript exporter deprecation warning and CoreML
unbounded-dimension diagnostics remain visible. The tests passed despite those
diagnostics; this is not evidence of an entirely GPU-resident pipeline or
real-camera quality. No model weights are committed.

## Skip and failure behavior

Reproduced the original failure first: with `TESSERA_TRAIN_PYTHON` unset,
`cargo test -p ml-enhance --release --test cfa_model -- --nocapture` failed on
the deleted M3-16 venv requirement.

Then exercised the new behavior with subprocess assertions on exit status and
output (each invocation used the same targeted cargo command):

| Environment | Expected and observed |
| --- | --- |
| No CI, interpreter variable unset | Exit 0, explicit unset-variable SKIP |
| No CI, nonexistent interpreter path | Exit 0, explicit missing-venv SKIP |
| CI=true, interpreter=/usr/bin/false | Exit 0, explicit CI SKIP before execution |
| CI empty, interpreter=/usr/bin/false | Exit 0, explicit CI SKIP before execution |
| No CI, interpreter=/usr/bin/false | Exit 101, calibration/training/export failure, no SKIP |
| No CI, real venv (absolute and repository-relative paths) | Full training/export/inference passes |

Rust counts early-return skips as passed tests, not ignored tests. Skip reasons
are printed to stderr and visible with `--nocapture`. Existing-but-broken
interpreters are deliberately not treated as missing environments.

## Required gates

Ran the exact requested command successfully after the final Rust change,
with the real venv enabled and `CI` unset:

```sh
cargo test -p ml-enhance --release && cargo clippy -p ml-enhance --all-targets -- -D warnings && cargo fmt --check
```

Exit status: 0. The CFA test trained/exported again, reporting 4.035 s for the
Python driver and 5.02 s for the full test. Other cached-model tests retain
their existing conditional behavior; their harness success is not a claim
that optional pretrained models were installed.

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-16c` was preserved
for every cargo invocation. No local `target/` directory was created. Cargo
added unrelated workspace packages to Cargo.lock during dependency resolution;
that generated lockfile change was reverted to keep the final diff in scope.

## Retry verification

Re-ran the documented venv setup and `pip check`, the full opt-in integration,
and the exact three-command gate above. Rechecked unset/missing interpreter,
CI=true, empty CI, and existing-but-failing interpreter behavior with assertions
on exit codes and messages. The final explicit integration output is retained
in `full-training.log` beside this report. The earlier first-run measurements
above are retained from the previous attempt.

The retry started with Cargo.lock already modified by the previous Cargo run.
After all Cargo invocations, restored only that generated lockfile change and
checked that every remaining changed/untracked path matches the allowed scope.
Subsequent Cargo runs may regenerate the unrelated workspace entries; the
lockfile is not part of this work package.

RESULT: PASS
