# SPDX-License-Identifier: MIT
"""Export owned CFA weights, pin opset 17, register hashes (never download)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import onnx
from onnxconverter_common import float16
import torch
from train_cfa_denoise import CfaNet


def export(checkpoint, output, manifest=None):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    manifest = Path(manifest) if manifest else output / 'models.toml'
    model = CfaNet().eval()
    model.load_state_dict(torch.load(checkpoint, map_location='cpu', weights_only=True))
    fp32 = output / 'cfa-fp32.onnx'
    torch.onnx.export(model, (torch.zeros(1, 8, 64, 64),), fp32,
                      input_names=['cfa_noise'], output_names=['denoised'],
                      opset_version=17, dynamo=False,
                      dynamic_axes={'cfa_noise': {2: 'height', 3: 'width'},
                                    'denoised': {2: 'height', 3: 'width'}})
    graph = onnx.load(fp32)
    onnx.checker.check_model(graph)
    half = float16.convert_float_to_float16(graph, keep_io_types=True)
    onnx.checker.check_model(half)
    onnx.save(half, output / 'cfa-fp16.onnx')
    entries = []
    for dtype in ['fp32', 'fp16']:
        path = output / f'cfa-{dtype}.onnx'
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        local = os.path.relpath(path.resolve(), manifest.resolve().parent)
        entries.append(f'''[[models]]
id = "enhance/cfa-unet-{dtype}"
version = "{sha}"
task = "cfa-denoise"
dtype = "{dtype}"
source = "local"
local_path = {json.dumps(local)}
sha256 = "{sha}"
inputs = [{{ name = "cfa_noise", shape = [1, 8, 64, 64], dtype = "fp32" }}]
outputs = [{{ name = "denoised", shape = [1, 4, 64, 64], dtype = "fp32" }}]
''')
    # Own only this explicit generated section. Existing registry stays intact.
    marker = '# BEGIN GENERATED CFA MODELS\n'
    end = '# END GENERATED CFA MODELS\n'
    previous = manifest.read_text() if manifest.exists() else ''
    if marker in previous:
        before, rest = previous.split(marker, 1)
        _, after = rest.split(end, 1)
    else:
        before, after = previous.rstrip() + '\n\n', ''
    manifest.write_text(before + marker + '\n'.join(entries) + end + after)
    print(f'Exported opset 17 fp32/fp16; hashes registered in {manifest}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('checkpoint', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    args = parser.parse_args()
    export(args.checkpoint, args.output, args.manifest)
