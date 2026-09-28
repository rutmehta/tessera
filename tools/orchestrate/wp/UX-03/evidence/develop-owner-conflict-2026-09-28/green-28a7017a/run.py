import os
import pathlib
import subprocess
import sys

root = pathlib.Path('/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera')
evidence = pathlib.Path('/Volumes/betterSSD/tessera-validation/develop-owner-conflict/green-28a7017a')
env = os.environ.copy()
env.update(CARGO_BUILD_JOBS='2', RAYON_NUM_THREADS='2', CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target')
names = [
    'open_develop_editor_does_not_overwrite_newer_engine_settings',
    'selection_during_develop_does_not_conflict_with_edited_settings',
    'legacy_rgb_disk_baseline_does_not_conflict_on_first_develop_save',
]
for name in names:
    command = ['cargo', 'test', '-p', 'tessera-ffi', '--lib', f'develop::tests::{name}', '--', '--exact', '--nocapture']
    (evidence / f'{name}.command.txt').write_text(' '.join(command) + '\n')
    with (evidence / f'{name}.log').open('wb') as log:
        try:
            outcome = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=300)
            code = outcome.returncode
        except subprocess.TimeoutExpired:
            code = 124
    (evidence / f'{name}.exit.txt').write_text(f'{code}\n')
    print(f'{name}: {code}', flush=True)
    if code == 124:
        sys.exit(code)
