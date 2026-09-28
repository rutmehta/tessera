import os
import pathlib
import subprocess
import sys

root = pathlib.Path('/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera')
evidence = pathlib.Path('/Volumes/betterSSD/tessera-validation/develop-owner-conflict/green-915c7f7b')
env = os.environ.copy()
env.update(CARGO_BUILD_JOBS='2', RAYON_NUM_THREADS='2', CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target')
names = [
    'failed_close_repairs_without_a_new_edit_and_only_then_drains_worker',
    'failed_close_retains_live_session_for_a_later_edit',
    'successful_close_rejects_retained_arc_mutations_and_flush',
    'concurrent_closes_share_one_failed_attempt_and_later_retry',
    'save_listener_cannot_self_join_or_self_flush',
    'temporary_depth_histogram_session_coexists_with_editor_and_closed_editor',
    'flush_without_new_edit_repairs_sidecars_after_recipe_write_failure',
    'failed_develop_sidecar_repair_waits_for_each_explicit_flush',
    'repair_uses_current_disk_recipe_after_foreign_develop_edit',
    'newer_develop_edit_does_not_hide_previous_save_failure',
    'concurrent_flushes_both_report_the_same_failed_save',
]
commands = [(f'adjacent-{name}', ['cargo', 'test', '-p', 'tessera-ffi', '--lib', f'develop::tests::{name}', '--', '--exact', '--nocapture']) for name in names]
commands += [
    ('recipe-write-tests', ['cargo', 'test', '-p', 'tessera-ffi', '--lib', 'recipe_write_tests', '--', '--nocapture']),
    ('cargo-fmt', ['cargo', 'fmt', '--all', '--', '--check']),
    ('cargo-clippy', ['cargo', 'clippy', '-p', 'tessera-ffi', '--all-targets', '--', '-D', 'warnings']),
]
for label, command in commands:
    (evidence / f'{label}.command.txt').write_text(' '.join(command) + '\n')
    with (evidence / f'{label}.log').open('wb') as log:
        try:
            outcome = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=300)
            code = outcome.returncode
        except subprocess.TimeoutExpired:
            code = 124
    (evidence / f'{label}.exit.txt').write_text(f'{code}\n')
    print(f'{label}: {code}', flush=True)
    if code:
        sys.exit(code)
