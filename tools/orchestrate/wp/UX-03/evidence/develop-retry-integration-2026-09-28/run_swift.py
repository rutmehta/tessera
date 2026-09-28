#!/usr/bin/env python3
import hashlib,json,os,pathlib,re,signal,subprocess,time
REPO=pathlib.Path('/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera')
HERE=pathlib.Path(__file__).resolve().parent
M=json.loads((HERE/'swift-manifest.json').read_text())
SCRATCH='/Volumes/betterSSD/tessera-cache/swiftpm/develop-retry-integration-release'
def verify():
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip()==M['head']
    assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all'],cwd=REPO)
    for name,meta in M['files'].items():
        assert hashlib.sha256((REPO/name).read_bytes()).hexdigest()==meta['sha256'],name
    assert hashlib.sha256((REPO/'apps/mac/build/ffi/libtessera_ffi.a').read_bytes()).hexdigest()==M['ffi_archive']['sha256']
def run(name,command,timeout,minimum):
    verify()
    env=os.environ.copy();env.pop('TESSERA_LAYOUT_DIAGNOSTICS',None)
    env.update(SWIFTPM_MAX_CONCURRENT_OPERATIONS='2',MACOSX_DEPLOYMENT_TARGET='15.0')
    start=time.time()
    with (HERE/f'{name}.log').open('wb') as log:
        process=subprocess.Popen(command,cwd=REPO/'apps/mac',env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try: code=process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL);process.wait();code='timeout'
    verify()
    output=(HERE/f'{name}.log').read_text(errors='replace')
    matches=re.findall(r'Executed (\d+) tests?, with (?:(\d+) test skipped and )?(\d+) failures?',output)
    count,skipped,failures=tuple(map(int,(matches[-1][0],matches[-1][1] or '0',matches[-1][2]))) if matches else (0,0,-1)
    accepted=code==0 and count>=minimum and failures==0
    swift_testing=5 if 'Test run with 5 tests in 2 suites passed' in output else 0
    if name=='full': accepted=accepted and skipped==1 and swift_testing==5
    (HERE/f'{name}.json').write_text(json.dumps({'command':command,'exit':code,'xctest_executed':count,'xctest_skipped':skipped,'xctest_failures':failures,'swift_testing_passed':swift_testing,'accepted':accepted,'source_head':M['head'],'ffi_sha256':M['ffi_archive']['sha256'],'elapsed_seconds':time.time()-start},indent=2)+'\n')
    print(name,code,count,skipped,failures,swift_testing,accepted,flush=True)
    return accepted
focused=['swift','test','-c','release','--jobs','2','--scratch-path',SCRATCH,'--filter','AgentReviewOwnershipTests|DevelopTests|AgentReviewLayoutTests']
if run('focused',focused,900,41):
    full=['swift','test','-c','release','--skip-build','--jobs','2','--scratch-path',SCRATCH]
    run('full',full,1200,516)
