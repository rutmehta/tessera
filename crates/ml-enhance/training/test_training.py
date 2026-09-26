# SPDX-License-Identifier: MIT
"""Calibration unittest and bounded CPU smoke driver used by cfa_model.rs.

See crates/ml-enhance/training/README.md for setup. Without --output, run
calibration tests only (also discoverable by pytest). With --output DIR, also
train 300 steps and export both ONNX variants for Rust quality/tiling checks.
"""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'tools'))


class TrainingTests(unittest.TestCase):
    def test_fit_recovers_poisson_gaussian_parameters(self):
        import numpy as np
        from train_cfa_denoise import fit_noise

        rng = np.random.default_rng(42)
        shot = np.array([0.002, 0.003, 0.004, 0.005])
        read = np.array([0.0005, 0.0008, 0.001, 0.0012])
        means = np.linspace(0.05, 0.85, 128)[:, None, None, None]
        shot_map = shot[None, :, None, None]
        read_map = read[None, :, None, None]
        shape = (128, 4, 64, 64)
        patches = rng.poisson(means / shot_map, size=shape) * shot_map
        patches += rng.normal(size=shape) * np.sqrt(read_map)
        actual_shot, actual_read = fit_noise(patches)
        np.testing.assert_allclose(actual_shot, shot, rtol=0.1, atol=0)
        np.testing.assert_allclose(actual_read, read, rtol=0.1, atol=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('tests', nargs='*')
    args = parser.parse_args()
    start = time.monotonic()
    suite = unittest.defaultTestLoader.loadTestsFromNames(
        args.tests or ['TrainingTests'], module=sys.modules[__name__])
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        return 1
    if args.output is not None:
        output = args.output.resolve()
        env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
        commands = [
            [sys.executable, str(ROOT / 'tools/train_cfa_denoise.py'),
             '--synthetic', '--steps', '300', '--device', 'cpu', '--output', str(output)],
            [sys.executable, str(ROOT / 'tools/export_cfa_denoise.py'),
             str(output / 'cfa.pt'), '--output', str(output)],
        ]
        for command in commands:
            remaining = 180 - (time.monotonic() - start)
            if remaining <= 0:
                raise TimeoutError('CFA calibration/training/export exceeded 180 seconds')
            subprocess.run(command, cwd=ROOT, env=env, check=True, timeout=remaining)
        elapsed = time.monotonic() - start
        if elapsed >= 180:
            raise TimeoutError('CFA calibration/training/export exceeded 180 seconds')
        print(f'CFA calibration/training/export: {elapsed:.3f}s', flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
