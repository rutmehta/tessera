# CFA training prerequisites

From the repository root, using Python 3.11–3.13 (tested with 3.13.3 on Apple
Silicon), create the isolated environment in one command:

```sh
python3 -m venv crates/ml-enhance/training/.venv && crates/ml-enhance/training/.venv/bin/python -m pip install -r crates/ml-enhance/training/requirements.txt
```

Direct dependencies are version-pinned: PyTorch and NumPy (BSD-style), ONNX
(Apache-2.0), ONNX Runtime and onnxconverter-common (MIT). PyTorch macOS wheels
support MPS; the tiny test deliberately uses CPU for reproducibility. The fp16
exporter requires onnxconverter-common. The standard-library unittest driver
needs no pytest install, though pytest can discover its calibration test.
These are direct-package licences, not a blanket licence claim for all bundled
or transitive dependencies. Real RAW training additionally needs rawpy (MIT,
with LibRaw LGPL/CDDL); it is not needed or installed for this synthetic test.

## Full Rust integration test

Keep the externally supplied `CARGO_TARGET_DIR` set (outside the repository).
Opt in explicitly and make sure `CI` is unset for local training:

```sh
export PYTHONDONTWRITEBYTECODE=1
export TESSERA_TRAIN_PYTHON="$PWD/crates/ml-enhance/training/.venv/bin/python"
cargo test -p ml-enhance --release --test cfa_model -- --nocapture
```

`cfa_model` prints a `SKIP cfa_model:` reason and returns successfully when `CI`
is present (even empty or `false`), `TESSERA_TRAIN_PYTHON` is unset, or its
interpreter path is missing. Rust's harness counts these early returns as
passes, not ignored tests; use `--nocapture` to see the reason. Set the variable
to an interpreter path (absolute or relative to the repository root), not a
command with arguments. With an existing
interpreter and no `CI`, calibration, dependencies, training, export, quality,
and tiling failures remain hard failures. A broken environment is not skipped.

The driver verifies recovery of per-plane Poisson-Gaussian parameters within
10%, trains 300 CPU steps on procedural 32×32 crops, and exports fp32 and fp16
ONNX models with fp32 IO. Calibration/training/export have a shared 180-second
budget, with subprocess timeouts for training/export. Rust checks at least 3 dB
PSNR gain over 24 independently seeded held-out crops, tiling equivalence, and
executed-provider reports. Temporary checkpoints, samples, manifests, and ONNX
files are removed when the Rust test finishes. This is a smoke test, not evidence
of real-camera denoising quality or GPU residency.

To run only calibration, or retain generated artifacts for inspection:

```sh
"$TESSERA_TRAIN_PYTHON" crates/ml-enhance/training/test_training.py TrainingTests.test_fit_recovers_poisson_gaussian_parameters
"$TESSERA_TRAIN_PYTHON" crates/ml-enhance/training/test_training.py --output crates/ml-enhance/training/artifacts
```

`.venv/` and `artifacts/` are ignored. Do not commit environments or weights.
See [TRAINING.md](../TRAINING.md) for the real-data method and limitations, and
`tools/orchestrate/wp/M3-16c/RESULTS.md` for measured smoke-test evidence.
