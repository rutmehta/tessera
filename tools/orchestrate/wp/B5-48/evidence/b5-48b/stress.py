import os, pathlib, subprocess, time
root = pathlib.Path(os.environ['CARGO_TARGET_DIR'])
binaries = [p for p in (root / 'debug/deps').glob('tessera_ffi-*') if p.is_file() and os.access(p, os.X_OK) and p.suffix == '']
binary = max(binaries, key=lambda p: p.stat().st_mtime)
name = 'document::render::frame_cancellation_tests::frames_for_a_replaced_ring_are_dropped'
print(f'Executable: {binary}', flush=True)
passed = 0
start = time.monotonic()
for i in range(1, 201):
    result = subprocess.run([str(binary), name, '--exact', '--nocapture'], capture_output=True, text=True, timeout=60)
    if result.returncode or '1 passed; 0 failed' not in result.stdout:
        print(f'FAIL iteration {i}:\n{result.stdout}\n{result.stderr}', flush=True)
        raise SystemExit(1)
    passed += 1
    print(f'{i}/200 PASS ({time.monotonic() - start:.2f}s)', flush=True)
print(f'RESULT: {passed}/200 passed, 0 flakes', flush=True)
