#!/usr/bin/env python3
"""Compare default, 8x, and 16x Core Graphics destination headroom per input."""
from pathlib import Path
import argparse, hashlib, json, os, plistlib, subprocess

HERE = Path(__file__).resolve().parent
PHASE = HERE.parent
REPO = PHASE.parents[6]
PHASE8 = REPO / 'tools/orchestrate/wp/EXP-45/evidence/2026-09-28/geometry-size-control-phase8/native/native-geometry-manifest.json'
INPUTS = {name: PHASE / 'inputs' / f'{name}.jpg' for name in ('A80', 'split80', 'uniform80')}
TARGETS = {'default': None, 'target8': '8', 'target16': '16'}

def sha_bytes(value): return hashlib.sha256(value).hexdigest()
def sha_file(path): return sha_bytes(path.read_bytes())

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--run-root', required=True, type=Path, help='new, empty directory on the validation volume')
    parser.add_argument('--binary', required=True, type=Path, help='compiled probe stored on the validation volume')
    args = parser.parse_args()
    out = args.run_root.resolve()
    assert not out.exists() or not any(out.iterdir()), f'run root must be empty: {out}'
    out.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve()
    source = HERE / 'imageio-probe-context.m'
    assert binary.is_file() and PHASE8.is_file()
    previous = json.loads(PHASE8.read_text())
    source_sha, binary_sha = sha_file(source), sha_file(binary)
    manifest = {
        'phase': 'phase10-context-edr-target-headroom',
        'probe_source_sha256': source_sha,
        'probe_binary_sha256': binary_sha,
        'phase8_probe_source_sha256': previous['imageio_source_sha256'],
        'phase8_probe_binary_sha256': previous['imageio_binary_sha256'],
        'phase8_manifest_sha256': sha_file(PHASE8),
        'targets': TARGETS,
        'inputs': {},
        'runs': {target: {} for target in TARGETS},
    }
    for name, source_path in INPUTS.items():
        assert source_path.is_file(), source_path
        source_digest = sha_file(source_path)
        assert source_digest == previous['inputs'][name]['sha256'], (name, 'fixture differs from phase8')
        manifest['inputs'][name] = {'relative_path': str(source_path.relative_to(PHASE)),
                                   'sha256': source_digest, 'bytes': source_path.stat().st_size,
                                   'dimensions_expected': [80, 16]}
        for target_name, target_value in TARGETS.items():
            run = out / target_name / name
            run.mkdir(parents=True, exist_ok=False)
            env = os.environ.copy()
            cleared = ['PROBE_OPTIONS', 'PROBE_LUMA_OFF', 'PROBE_TARGET_ZERO',
                       'PROBE_TARGET_HEADROOM', 'PROBE_HDR_STATS', 'PROBE_COMPUTE_HDR_STATS', 'PROBE_OUTPUT_DIR',
                       'PROBE_DUMP', 'PROBE_RAW']
            for key in cleared: env.pop(key, None)
            env.update({'PROBE_OUTPUT_DIR': str(run), 'PROBE_DUMP': str(run),
                        'PROBE_RAW': str(run / 'provider')})
            if target_value is not None: env['PROBE_TARGET_HEADROOM'] = target_value
            controlled = {k: env[k] for k in ['PROBE_OUTPUT_DIR', 'PROBE_DUMP', 'PROBE_RAW']}
            controlled['PROBE_TARGET_HEADROOM'] = target_value if target_value is not None else 'unset'
            argv = [str(binary), str(source_path)]
            command_record = {'argv': argv, 'cwd': str(PHASE),
                'controlled_env': controlled, 'cleared_inherited': cleared,
                'timeout_seconds': 60}
            (run / 'command.json').write_text(json.dumps(command_record, indent=2) + '\n')
            before_hashes = {'source_sha256': sha_file(source), 'binary_sha256': sha_file(binary),
                             'input_sha256': sha_file(source_path)}
            (run / 'attempt-freeze-before.json').write_text(json.dumps(before_hashes, indent=2) + '\n')
            try:
                completed = subprocess.run(argv, cwd=PHASE, env=env, capture_output=True, timeout=60)
            except subprocess.TimeoutExpired as timeout:
                partial_out = timeout.stdout or b''
                partial_err = timeout.stderr or b''
                if isinstance(partial_out, str): partial_out = partial_out.encode()
                if isinstance(partial_err, str): partial_err = partial_err.encode()
                (run / 'stdout.partial.bin').write_bytes(partial_out)
                (run / 'stderr.partial.bin').write_bytes(partial_err)
                (run / 'direct.exit').write_text('timeout after 60 seconds\n')
                after_hashes = {'source_sha256': sha_file(source), 'binary_sha256': sha_file(binary),
                                'input_sha256': sha_file(source_path)}
                (run / 'attempt-freeze-after.json').write_text(json.dumps(after_hashes, indent=2) + '\n')
                raise
            (run / 'stdout.bin').write_bytes(completed.stdout)
            (run / 'stderr.bin').write_bytes(completed.stderr)
            (run / 'direct.exit').write_text(f'{completed.returncode}\n')
            after_hashes = {'source_sha256': sha_file(source), 'binary_sha256': sha_file(binary),
                            'input_sha256': sha_file(source_path)}
            (run / 'attempt-freeze-after.json').write_text(json.dumps(after_hashes, indent=2) + '\n')
            assert before_hashes == after_hashes, (name, target_name, 'probe/binary/fixture changed during process')
            start = completed.stdout.find(b'[\n')
            assert start >= 0, (name, target_name, completed.stdout[:300], completed.stderr)
            warning = completed.stdout[:start]
            (run / 'warning-prefix.txt').write_bytes(warning)
            records = json.loads(completed.stdout[start:])
            assert completed.returncode == 0 and len(records) == 1, (name, target_name, completed.returncode, records, completed.stderr)
            record = records[0]
            assert record.get('image_count') == 1 and record['path'] == str(source_path)
            assert record['source_properties_plist'] == str(run / 'source-properties.plist')
            props_path = Path(record['source_properties_plist'])
            assert props_path.is_file() and isinstance(plistlib.loads(props_path.read_bytes()), dict)
            record['source_properties_sha256'] = sha_file(props_path)
            record['source_properties_bytes'] = props_path.stat().st_size
            assert sha_file(source_path) == source_digest
            for mode in ['sdr', 'hdr']:
                image = record[mode]
                assert image['image_created'] and image['pixels_decoded']
                assert (image['width'], image['height']) == (80, 16)
                assert image['nonfinite_rgb'] == 0 and image['alpha_min'] > .99 and image['alpha_max'] < 1.01
                expected = previous['runs']['imageio'][name][mode]
                expected_target = 0.0 if target_value is None else float(target_value)
                assert abs(image['context_target_after'] - expected_target) < 1e-6
                assert abs(image['context_target_before']) < 1e-6
                assert image['context_target_requested_set'] is (target_value is not None)
                assert image['context_target_set'] is (target_value is not None)
                if target_value is not None:
                    assert abs(image['context_target_requested'] - expected_target) < 1e-6
                # Decoding identity is held constant across context targets.
                assert image['provider_bytes'] == expected['provider_bytes']
                assert image['provider_sha256'] == expected['provider_sha256']
                assert image['input_icc_bytes'] == expected['input_icc_bytes']
                assert image['input_icc_sha256'] == expected['input_icc_sha256']
                assert image['headroom'] == expected['headroom']
                for field in ['color_space', 'input_color_space_present', 'input_color_space_model',
                              'input_color_space_components', 'bits_per_component', 'bits_per_pixel',
                              'bytes_per_row', 'bitmap_info']:
                    assert image[field] == expected[field], (name, target_name, mode, field,
                                                              image[field], expected[field])
                provider = run / f'provider-{mode}.bin'
                pixels = run / f'{source_path.name}.{mode}.f32'
                icc = run / f'{mode}-returned-colorspace-icc.icc'
                for artifact in [provider, pixels, icc]: assert artifact.is_file(), artifact
                assert provider.stat().st_size == image['provider_bytes'] and sha_file(provider) == image['provider_sha256']
                assert pixels.stat().st_size == 80 * 16 * 16
                assert icc.stat().st_size == image['input_icc_bytes'] and sha_file(icc) == image['input_icc_sha256']
                image['artifacts'] = {
                    'provider': {'bytes': provider.stat().st_size, 'sha256': sha_file(provider)},
                    'drawn_rgba_f32': {'bytes': pixels.stat().st_size, 'sha256': sha_file(pixels)},
                    'returned_icc': {'bytes': icc.stat().st_size, 'sha256': sha_file(icc)},
                }
                image['phase8_draw_hash'] = expected['drawn_rgba_f32']['sha256']
            (run / 'result.json').write_text(json.dumps(record, indent=2) + '\n')
            manifest['runs'][target_name][name] = {
                'direct_exit': completed.returncode,
                'warning_prefix': warning.decode('utf-8', 'replace').strip().splitlines(),
                'warning_prefix_sha256': sha_bytes(warning),
                'record': record,
            }
            if target_name == 'default':
                # Stop before issuing any nonzero-target process unless this probe's
                # no-setter path reproduces the retained phase8 result for this file.
                for mode in ['sdr', 'hdr']:
                    old = previous['runs']['imageio'][name][mode]
                    new = record[mode]
                    assert new['provider_sha256'] == old['provider_sha256']
                    assert new['input_icc_sha256'] == old['input_icc_sha256']
                    assert new['headroom'] == old['headroom']
                    assert abs(new['rgb_max'] - old['rgb_max']) < 1e-5
                    assert record[mode]['artifacts']['drawn_rgba_f32']['sha256'] == old['drawn_rgba_f32']['sha256']
                for field in ['iso_aux_present', 'apple_aux_present']:
                    assert record[field] == previous['runs']['imageio'][name][field]
    # Compare requested targets against that same-run default without prescribing a peak.
    for name in INPUTS:
        default = manifest['runs']['default'][name]['record']
        for target_name in ['target8', 'target16']:
            target = manifest['runs'][target_name][name]['record']
            target['delta_vs_default'] = {}
            for mode in ['sdr', 'hdr']:
                a, b = default[mode], target[mode]
                target['delta_vs_default'][mode] = {
                    'provider_identical': a['artifacts']['provider']['sha256'] == b['artifacts']['provider']['sha256'],
                    'icc_identical': a['artifacts']['returned_icc']['sha256'] == b['artifacts']['returned_icc']['sha256'],
                    'same_headroom': a['headroom'] == b['headroom'],
                    'draw_identical': a['artifacts']['drawn_rgba_f32']['sha256'] == b['artifacts']['drawn_rgba_f32']['sha256'],
                    'rgb_max_delta': b['rgb_max'] - a['rgb_max'],
                    'rgb_mean_delta': b['rgb_mean'] - a['rgb_mean'],
                    'samples_above_one_delta': b['samples_above_one'] - a['samples_above_one'],
                }
    (out / 'context-headroom-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({name: {target: {
        'hdr_peak': manifest['runs'][target][name]['record']['hdr']['rgb_max'],
        'headroom': manifest['runs'][target][name]['record']['hdr']['headroom'],
        'provider_sha256': manifest['runs'][target][name]['record']['hdr']['provider_sha256'],
        'icc_sha256': manifest['runs'][target][name]['record']['hdr']['input_icc_sha256'],
    } for target in TARGETS} for name in INPUTS}, indent=2))

if __name__ == '__main__': main()
