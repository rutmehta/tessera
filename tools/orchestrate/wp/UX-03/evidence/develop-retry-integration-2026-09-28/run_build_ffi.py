#!/usr/bin/env python3
import hashlib, json, os, pathlib, signal, subprocess, time
REPO=pathlib.Path('/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera')
HERE=pathlib.Path(__file__).resolve().parent
SOURCE=HERE/'manifest.json'
M=json.loads(SOURCE.read_text())
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip()==M['head']
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all'],cwd=REPO)
for path,meta in M['files'].items():
    assert hashlib.sha256((REPO/path).read_bytes()).hexdigest()==meta['sha256'],path
archive=pathlib.Path(json.loads((HERE.parent/'prior-archive.json').read_text())['backup'])
old=REPO/'apps/mac/build/ffi/libtessera_ffi.a'
assert not old.is_symlink()
assert hashlib.sha256(old.read_bytes()).digest()==hashlib.sha256(archive.read_bytes()).digest()
command=['bash','apps/mac/build-ffi.sh']
env=os.environ.copy()
env.update(CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/main',CARGO_BUILD_JOBS='2',RAYON_NUM_THREADS='2',MACOSX_DEPLOYMENT_TARGET='15.0')
start=time.time()
with (HERE/'build-ffi.log').open('wb') as log:
    process=subprocess.Popen(command,cwd=REPO,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    try: code=process.wait(timeout=1800)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid,signal.SIGKILL);process.wait();code='timeout'
(HERE/'build-ffi.json').write_text(json.dumps({'command':command,'exit':code,'started_unix':start,'elapsed_seconds':time.time()-start,'source_head':M['head'],'target_dir':env['CARGO_TARGET_DIR']},indent=2)+'\n')
print('build-ffi',code,flush=True)
