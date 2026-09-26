#!/usr/bin/env python3
"""Gated local GFPGAN v1.4 export; NOT a license clearance or downloader.

The export path remains unvalidated until the license blocker is resolved.
Only writes below training/.cache; never edits the production registry.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

SOURCE_REVISION = '7552a7791caad982045a7bbe5634bbf1cd5c8679'
CHECKPOINT_SHA256 = 'e2cd4703ab14f4d01fd1383a8a8b266f9a5833dacee8e6a79d3bf21a1b6be5ad'
CACHE = Path(__file__).resolve().parent / '.cache'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--license-review', type=Path,
                        help='JSON documenting approval of this exact graph and checkpoint')
    parser.add_argument('--source', type=Path, help='Clean pinned TencentARC/GFPGAN checkout')
    parser.add_argument('--checkpoint', type=Path, help='Local verified GFPGANv1.4.pth')
    args = parser.parse_args()
    if args.license_review is None:
        parser.exit(2, 'BLOCKED: GFPGAN license review required; see ../MODELS.md\n')
    review = json.loads(args.license_review.read_text())
    required = {
        'source_revision': SOURCE_REVISION,
        'checkpoint_sha256': CHECKPOINT_SHA256,
        'approved': True,
    }
    if (any(review.get(k) != v for k, v in required.items())
            or review.get('license') not in {'Apache-2.0', 'MIT', 'BSD-2-Clause', 'BSD-3-Clause'}
            or not review.get('reviewer') or not review.get('third_party_scope_analysis')):
        parser.exit(2, 'BLOCKED: incomplete or mismatched license review\n')
    if args.source is None or args.checkpoint is None:
        parser.error('--source and --checkpoint are required after license review')
    source = args.source.resolve()
    revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    dirty = subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True)
    if revision != SOURCE_REVISION or dirty:
        parser.error('source must be the clean pinned upstream revision')
    with args.checkpoint.open('rb') as f:
        digest = hashlib.file_digest(f, 'sha256').hexdigest()
    if digest != CHECKPOINT_SHA256:
        parser.error('checkpoint SHA-256 mismatch')

    # Heavy imports deliberately follow all provenance and hash checks.
    sys.path.insert(0, str(source))
    import numpy as np
    import onnx
    import onnxruntime as ort
    import torch
    from gfpgan.archs.gfpganv1_clean_arch import GFPGANv1Clean

    torch.manual_seed(0)
    torch.set_num_threads(2)
    model = GFPGANv1Clean(
        out_size=512, num_style_feat=512, channel_multiplier=2,
        decoder_load_path=None, fix_decoder=False, num_mlp=8,
        input_is_latent=True, different_w=True, narrow=1, sft_half=True,
    ).cpu().eval()
    checkpoint = torch.load(args.checkpoint, map_location='cpu', weights_only=True)
    model.load_state_dict(checkpoint['params_ema'], strict=True)

    class ImageOnly(torch.nn.Module):
        def __init__(self, network):
            super().__init__()
            self.network = network

        def forward(self, image):
            return self.network(image, return_rgb=False, randomize_noise=False)[0]

    wrapper = ImageOnly(model).eval()
    example = torch.linspace(-1, 1, 512).reshape(1, 1, 1, 512).expand(1, 3, 512, 512).contiguous()
    CACHE.mkdir(parents=True, exist_ok=True)
    temporary = CACHE / 'gfpgan-v1.4.partial.onnx'
    destination = CACHE / 'gfpgan-v1.4.onnx'
    with torch.no_grad():
        reference = wrapper(example).numpy()
        torch.onnx.export(wrapper, (example,), str(temporary), opset_version=17,
                          input_names=['input'], output_names=['output'],
                          dynamo=False, do_constant_folding=True)
    graph = onnx.load(temporary)
    onnx.checker.check_model(graph)
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    session = ort.InferenceSession(str(temporary), options, providers=['CPUExecutionProvider'])
    actual = session.run(['output'], {'input': example.numpy()})[0]
    np.testing.assert_allclose(actual, reference, rtol=1e-3, atol=1e-3)
    if actual.shape != (1, 3, 512, 512) or not np.isfinite(actual).all():
        raise ValueError('Invalid output contract')
    temporary.replace(destination)
    with destination.open('rb') as f:
        output_hash = hashlib.file_digest(f, 'sha256').hexdigest()
    report = {
        'source_revision': revision, 'checkpoint_sha256': digest,
        'sha256': output_hash, 'size_bytes': destination.stat().st_size,
        'license_review': review, 'torch': torch.__version__, 'onnx': onnx.__version__,
        'onnxruntime': ort.__version__, 'opset': 17,
        'inputs': [{'name': 'input', 'shape': [1, 3, 512, 512], 'dtype': 'fp32'}],
        'outputs': [{'name': 'output', 'shape': [1, 3, 512, 512], 'dtype': 'fp32'}],
        'max_abs_error': float(np.max(np.abs(actual - reference))),
    }
    destination.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
