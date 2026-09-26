# SPDX-License-Identifier: MIT
"""Small CFA training utilities. See crates/ml-enhance/TRAINING.md."""
import numpy as np
import argparse
import hashlib
import json
import time
from pathlib import Path
import torch
from torch import nn
from torch.nn import functional as F


class CfaNet(nn.Module):
    """One-level local U-Net, 8 channels in (CFA + per-plane sigma), 4 out.

    Two 3x3 convolutions per level, stride-2 down, nearest up, no global ops.
    Conservative packed-pixel radius 16, alignment 2. Linear unbounded output.
    """
    def __init__(self):
        super().__init__()
        self.enc = nn.Sequential(nn.Conv2d(8, 12, 3, padding=1), nn.ReLU(),
                                 nn.Conv2d(12, 12, 3, padding=1), nn.ReLU())
        self.low = nn.Sequential(nn.Conv2d(12, 24, 3, stride=2, padding=1), nn.ReLU(),
                                 nn.Conv2d(24, 24, 3, padding=1), nn.ReLU())
        self.dec = nn.Sequential(nn.Conv2d(36, 12, 3, padding=1), nn.ReLU(),
                                 nn.Conv2d(12, 4, 3, padding=1))
        nn.init.zeros_(self.dec[-1].weight)
        nn.init.zeros_(self.dec[-1].bias)

    def forward(self, x):
        skip = self.enc(x)
        low = F.interpolate(self.low(skip), scale_factor=2, mode='nearest')
        return x[:, :4] + self.dec(torch.cat([skip, low], 1))


def synthetic_crops(count, seed, side=32):
    """Procedural smooth colour fields with hard rectangular edges; test only."""
    g = torch.Generator().manual_seed(seed)
    base = torch.rand(count, 4, 6, 6, generator=g)
    clean = F.interpolate(base, size=(side, side), mode='bilinear', align_corners=False)
    for i in range(count):
        y, x = torch.randint(4, side - 8, (2,), generator=g).tolist()
        clean[i, :, y:y + 6, x:x + 8] = torch.rand(4, 1, 1, generator=g)
    return clean


def load_raw_crops(root, side=32, allow_user_raws=False):
    """Rotate complete sensor images to RGGB, then pack, no demosaic or WB."""
    import rawpy
    known = {'canon-cr3.CR3', 'sony-arw.ARW', 'nikon-nef.NEF', 'fuji-raf.RAF', 'sample.dng'}
    rng = np.random.default_rng(12)
    training, heldout, records = [], [], []
    for path in sorted(Path(root).iterdir()):
        if not path.is_file() or path.suffix.lower() not in {'.cr3', '.arw', '.nef', '.raf', '.dng', '.cr2', '.rw2'}:
            continue
        if path.name not in known and not allow_user_raws:
            raise ValueError(f'{path}: pass --allow-user-raws only for owned/permissive data')
        with rawpy.imread(str(path)) as raw:
            pattern = raw.raw_pattern
            if pattern is None or pattern.shape != (2, 2):
                records.append({'file': path.name, 'skipped': 'non-Bayer; RGB fallback'})
                continue
            data = raw.raw_image_visible.astype(np.float32)
            colors = raw.raw_colors_visible
            black = np.array(raw.black_level_per_channel, dtype=np.float32)[colors]
            data = (data - black) / (raw.white_level - black)
            # Even dimensions preserve phase under 90-degree rotations.
            data = data[:data.shape[0] // 2 * 2, :data.shape[1] // 2 * 2]
            labels = np.array([chr(v) for v in raw.color_desc])[pattern]
            for turns in range(4):
                if np.array_equal(np.rot90(labels, turns), [['R', 'G'], ['G', 'B']]):
                    data = np.rot90(data, turns)
                    break
            else:
                raise ValueError(f'{path}: unsupported Bayer labels {labels}')
            packed = np.stack([data[0::2, 0::2], data[0::2, 1::2],
                               data[1::2, 0::2], data[1::2, 1::2]])
        # Disjoint top/bottom spatial split with a guard band. Never split noisy
        # variants of a single clean crop between training and evaluation.
        h, w = packed.shape[-2:]
        flat = []
        for split, dest in [(0, training), (1, heldout)]:
            low, high = (0, h // 2 - side) if split == 0 else (h // 2 + side, h - side)
            for _ in range(96 if split == 0 else 24):
                y, x = rng.integers(low, high), rng.integers(0, w - side)
                crop = packed[:, y:y + side, x:x + side].copy()
                if crop.min() < 0 or crop.max() > 0.95:
                    continue
                dest.append(torch.from_numpy(crop))
                if split == 0:
                    # Coarse block-mean spread selects low-texture patches.
                    spread = crop.reshape(4, 4, side // 4, 4, side // 4).mean((2, 4)).var((1, 2)).mean()
                    flat.append((spread, crop))
        selected = [c for _, c in sorted(flat, key=lambda x: x[0])[:32]]
        try:
            shot, read = fit_noise(np.stack(selected))
            fit = {'shot': shot.tolist(), 'read': read.tolist(), 'method': 'flat-patch OLS (texture-biased estimate)'}
        except ValueError as error:
            fit = {'unavailable': str(error)}
        records.append({'file': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                        'rotation': turns, 'noise': fit})
    if not training or not heldout:
        raise ValueError('no usable Bayer crops')
    return torch.stack(training), torch.stack(heldout), records


def corrupt(clean, g, shot=0.003, read=0.0008):
    """Poisson shot counts plus independent Gaussian read noise, no clipping."""
    noisy = torch.poisson(clean.clamp_min(0) / shot, generator=g) * shot
    noisy += torch.randn(clean.shape, generator=g) * read ** 0.5
    sigma = (noisy.clamp_min(0) * shot + read).sqrt()
    return torch.cat([noisy, sigma], 1)


def neighbor_pair(noisy, g):
    """Independent adjacent samples per packed colour, no cross-colour mixing."""
    # Random horizontal/vertical adjacent subimage pair, same choice per crop.
    if torch.rand((), generator=g) < 0.5:
        return noisy[:, :, 0::2, 0::2], noisy[:, :, 0::2, 1::2]
    return noisy[:, :, 0::2, 0::2], noisy[:, :, 1::2, 0::2]


def train(output, steps=1200, synthetic=False, device=None, raw_root='fixtures/raw', allow_user_raws=False):
    torch.set_num_threads(2)
    torch.manual_seed(7)
    g = torch.Generator().manual_seed(8)
    device = device or ('mps' if torch.backends.mps.is_available() else 'cpu')
    start = time.monotonic()
    if synthetic:
        clean, test = synthetic_crops(96, 101), synthetic_crops(24, 202)
        records = [{'source': 'procedural smoke-test only; not camera quality evidence'}]
    else:
        clean, test, records = load_raw_crops(raw_root, allow_user_raws=allow_user_raws)
    calibrations = [r['noise'] for r in records if 'shot' in r.get('noise', {})]
    model = CfaNet().to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=0.002 if synthetic else 0.0005)
    for step in range(steps):
        target = clean[torch.randint(len(clean), (8,), generator=g)]
        conditioned = corrupt(target, g)
        if calibrations and step % 2 == 1:
            calibration = calibrations[(step // 2) % len(calibrations)]
            # Fitted camera profiles define additional injected noise levels.
            # Keep the fixed synthetic severity on alternate steps as a known
            # reference, not as a claimed camera/ISO calibration.
            strength = 1.0 + 31.0 * torch.rand((), generator=g).item()
            shot = torch.tensor(calibration['shot'], dtype=torch.float32).reshape(1,4,1,1).clamp_min(1e-7) * strength
            read = torch.tensor(calibration['read'], dtype=torch.float32).reshape(1,4,1,1).clamp_min(1e-8) * strength
            conditioned = corrupt(target, g, shot, read)
        if not synthetic and step % 4 == 0:
            # Neighbor2Neighbor-style self-supervision on the ORIGINAL sensor
            # samples (not synthetic noise), with independent neighbouring sites.
            a, b = neighbor_pair(target, g)
            sigma = (a.clamp_min(0) * 0.003 + 0.0008).sqrt()
            conditioned, target = torch.cat([a, sigma], 1), b
        prediction = model(conditioned.to(device))
        loss = F.mse_loss(prediction, target.to(device))
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step % 200 == 0:
            print(f'step={step} loss={loss.item():.7f}', flush=True)
    model = model.cpu().eval()
    probe = corrupt(test, torch.Generator().manual_seed(303))
    with torch.no_grad():
        result = model(probe)
    before = F.mse_loss(probe[:, :4], test).item()
    after = F.mse_loss(result, test).item()
    report = {'steps': steps, 'device': device, 'seconds': time.monotonic() - start,
              'input_psnr': -10 * np.log10(before), 'output_psnr': -10 * np.log10(after),
              'gain_db': 10 * np.log10(before / after), 'sources': records,
              'qualification': 'synthetic noise on held-out crops; original raw is a noisy proxy, not ground truth'}
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    torch.save(model.state_dict(), output / 'cfa.pt')
    test.numpy().astype('<f4').tofile(output / 'clean.f32')
    probe[:, :4].numpy().astype('<f4').tofile(output / 'noisy.f32')
    (output / 'training.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2), flush=True)
    return report


def fit_noise(patches):
    """OLS variance = shot*mean + read for preselected flat NCHW patches.

    Use same-colour samples only; texture biases the estimate upward. Reject
    insufficient intensity range instead of inventing a camera calibration.
    Returns shot, read arrays in normalized sensor units, not standard deviations.
    """
    patches = np.asarray(patches, dtype=np.float64)
    if patches.ndim != 4 or patches.shape[1] != 4 or not np.isfinite(patches).all():
        raise ValueError('finite N,4,H,W patches required')
    means = patches.mean(axis=(-2, -1))
    variances = patches.var(axis=(-2, -1), ddof=1)
    if len(means) < 3 or np.any(np.ptp(means, axis=0) < 0.05):
        raise ValueError('flat patches need at least 0.05 intensity range')
    shot, read = [], []
    for c in range(4):
        a, b = np.linalg.lstsq(np.stack([means[:, c], np.ones(len(means))], 1),
                             variances[:, c], rcond=None)[0]
        shot.append(max(0.0, a))
        read.append(max(0.0, b))
    return np.array(shot), np.array(read)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--steps', type=int, default=1200)
    parser.add_argument('--synthetic', action='store_true')
    parser.add_argument('--device', choices=['cpu', 'mps'])
    parser.add_argument('--raw-root', default='fixtures/raw')
    parser.add_argument('--allow-user-raws', action='store_true')
    args = parser.parse_args()
    train(args.output, args.steps, args.synthetic, args.device, args.raw_root, args.allow_user_raws)
