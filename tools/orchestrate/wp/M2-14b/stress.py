"""Run ten complete Swift suites while Rust workspace tests run continuously."""
import json
import os
from pathlib import Path
import subprocess
import threading
import time

root = Path(__file__).resolve().parents[4]
logs = root / 'tools/orchestrate/wp/M2-14b'
env = dict(os.environ, CARGO_TARGET_DIR='/Users/rutmehta/.cache/tessera-target/M2-14b',
           CARGO_INCREMENTAL='0', MACOSX_DEPLOYMENT_TARGET='15.0', RUST_TEST_THREADS='1')
stop = threading.Event()
started = threading.Event()
rust = []
swift = []
def load():
    while not stop.is_set():
        n = len(rust) + 1
        with (logs / f'rust-load-{n}.log').open('w') as out:
            start = time.time()
            p = subprocess.Popen(['cargo', 'test', '--workspace', '--release'], cwd=root, env=env, stdout=out, stderr=subprocess.STDOUT)
            started.set()
            p.wait()
            rust.append(dict(run=n, code=p.returncode, start=start, end=time.time()))
        if p.returncode:
            stop.set()
            break
worker = threading.Thread(target=load)
worker.start()
if not started.wait(timeout=30):
    stop.set()
    raise RuntimeError('Rust load did not start')
try:
    for n in range(1, 11):
        with (logs / f'swift-load-{n}.log').open('w') as out:
            start = time.time()
            p = subprocess.run(['swift', 'test'], cwd=root / 'apps/mac', env=env, stdout=out, stderr=subprocess.STDOUT)
            swift.append(dict(run=n, code=p.returncode, start=start, end=time.time()))
            print(f'Swift run {n}: exit {p.returncode}', flush=True)
finally:
    stop.set()
    worker.join()
    result = dict(swift=swift, rust=rust)
    (logs / 'stress-results.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result), flush=True)
raise SystemExit(0 if len(swift) == 10 and all(r['code'] == 0 for r in swift + rust) else 1)
