# Neural-filter acquisition and export conventions

See [../MODELS.md](../MODELS.md) for exact artifact evidence and the GFPGAN
exclusion (round-2 scope: StyleGAN2/NVIDIA non-commercial lineage). The historical
GFPGAN exporter is not part of the shipped feature set and is not authorization
to register or run that model. Existing Remove/LaMa guidance is in
`../../filters/training/README.md`; that separate pipeline is unchanged.

## Rules

1. Accept Apache-2.0, MIT, BSD-2-Clause, or BSD-3-Clause only. Review the actual
   weights/export and inherited third-party scope, not only a repository tag.
2. Pin immutable revision URLs and verify full SHA-256 against publisher LFS
   pointers. Hash the actual download too when available. Never invent a hash,
   reuse a PyTorch hash for ONNX, or register an unavailable planned export.
3. Inspect graph I/O and opset. Record weights precision separately from I/O
   precision, exact color space, scaling, alignment, and postprocessing.
4. Run ONNX checker and a real inference smoke test; separately label untested
   execution providers, quality metrics, and parity checks.
5. Keep dependencies, checkpoints, downloaded models, temporary exports, and
   local provenance in `.venv/` or `.cache/` below this directory (gitignored).
   Do not commit weights or force-add ignored artifacts. Store license/size
   metadata in registry comments; do not invent ModelSpec fields.
6. Export to a temporary path and publish only after checking/parity. Hash final
   bytes after all transformations. Keep source checkpoint hash distinct.
7. Model acquisition must be explicit. No imports or default tests download
   models. Production activation requires adapter integration and its tests;
   adding a registry entry alone does not deliver that integration.

## Reproduce DDColor verification

From the repository root:

```sh
python3 -m venv crates/ml-filters/training/.venv
crates/ml-filters/training/.venv/bin/pip install --only-binary=:all: onnx==1.19.0 onnxruntime==1.22.1
mkdir -p crates/ml-filters/training/.cache
curl -fL --retry 2 \
  'https://huggingface.co/edgetools/ddcolor/resolve/4755ae9f1f7a35a9e7693b96c2a88f3432cb6ab0/ddcolor-tiny-fp16.onnx' \
  -o crates/ml-filters/training/.cache/ddcolor-tiny-fp16.onnx
crates/ml-filters/training/.venv/bin/python - <<'PY'
import hashlib
from pathlib import Path
import tomllib
import onnx
import onnxruntime as ort
import numpy as np
p = Path('crates/ml-filters/training/.cache/ddcolor-tiny-fp16.onnx')
registry = tomllib.loads(Path('crates/ml-runtime/models.toml').read_text())
spec = next(m for m in registry['models'] if m['id'] == 'filters/ddcolor')
with p.open('rb') as f:
    assert hashlib.file_digest(f, 'sha256').hexdigest() == spec['sha256']
assert p.stat().st_size == 135444402
model = onnx.load(p)
onnx.checker.check_model(model)
for label, values in [('inputs', model.graph.input), ('outputs', model.graph.output)]:
    observed = [{'name': t.name,
                 'shape': [d.dim_value for d in t.type.tensor_type.shape.dim],
                 'dtype': 'fp32' if t.type.tensor_type.elem_type == 1 else 'unexpected'}
                for t in values]
    assert observed == spec[label], (label, observed)
options = ort.SessionOptions()
options.intra_op_num_threads = 2
options.log_severity_level = 3
session = ort.InferenceSession(str(p), options, providers=['CPUExecutionProvider'])
x = np.broadcast_to(np.linspace(0, 1, 512, dtype=np.float32)[None,None,None,:],
                    (1,3,512,512)).copy()
y = session.run(['output'], {'input': x})[0]
assert y.shape == (1,2,512,512) and y.dtype == np.float32 and np.isfinite(y).all()
print('PASS: hash, bytes, ONNX checker, registry I/O, finite CPU inference', y.min(), y.max())
PY
python3 crates/ml-filters/training/test_tools.py
```

Python 3.11+ supplies `tomllib` and `hashlib.file_digest`. On this Python 3.13
macOS environment ONNX 1.17.0 fell back to a failing native build; 1.19.0 has a
working wheel. Use `--only-binary=:all:` to fail quickly rather than accidentally
compiling the older package. These verification pins are not GFPGAN export pins.

## GFPGAN fallback after clearance only

No approved review exists in this repository. The script requires a review JSON
containing `approved: true`, `reviewer`, an allowlisted `license`, a substantive
`third_party_scope_analysis`, and the exact `source_revision` and
`checkpoint_sha256` documented in MODELS.md. The JSON is a human review record,
not a machine-generated legal conclusion. Do not create it merely to bypass the
block. Keep the checkpoint/source checkout under `.cache/`.

After genuine clearance and installation of a validated compatible export
environment (PyTorch legacy exporter with `dynamo=False`, GFPGAN/BasicSR,
torchvision, NumPy, ONNX and ONNX Runtime):

```sh
python crates/ml-filters/training/export_gfpgan.py \
  --license-review crates/ml-filters/training/.cache/gfpgan-license-review.json \
  --source crates/ml-filters/training/.cache/GFPGAN \
  --checkpoint crates/ml-filters/training/.cache/GFPGANv1.4.pth
```

Full export is **not yet exercised**. On success it creates `.cache/gfpgan-v1.4.onnx`
and `.cache/gfpgan-v1.4.json` with actual digest, size, I/O, versions, review, and
parity error. A future production entry additionally needs an immutable approved
published URL (or explicit local-model distribution), real hash and adapter tests.
