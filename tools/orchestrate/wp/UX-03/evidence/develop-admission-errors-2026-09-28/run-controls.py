import hashlib,json,os,pathlib,shlex,subprocess,time
root=pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
base=pathlib.Path('/Volumes/betterSSD/tessera-validation/develop-admission-error-8cc6bc18')
cache=pathlib.Path('/Volumes/betterSSD/tessera-cache/swift/develop-admission-error-8cc6bc18')
base.mkdir(parents=True,exist_ok=True)
cache.mkdir(parents=True,exist_ok=True)
def git(*args):return subprocess.check_output(['git',*args],cwd=root,text=True).strip()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
provenance=json.loads(pathlib.Path('/Volumes/betterSSD/tessera-validation/recipe-xmp-parity/ffi-4a45f823/provenance.json').read_text())
inputs={'libtessera_ffi.a':'apps/mac/build/ffi/libtessera_ffi.a','TesseraFFI.swift':'apps/mac/Sources/TesseraFFI/TesseraFFI.swift','CTesseraFFI.h':'apps/mac/Sources/CTesseraFFI/CTesseraFFI.h','CTesseraFFI.modulemap':'apps/mac/Sources/CTesseraFFI/module.modulemap'}
for key,p in inputs.items():assert sha(root/p)==provenance['archive_and_bindings'][key],p
(base/'accepted-archive-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
assert git('rev-parse','HEAD')=='8cc6bc18068f54843d4445e2d0549909623e51aa'
assert not git('status','--porcelain')
tracked=git('ls-files','apps/mac').splitlines()
tracked=[p for p in tracked if p.endswith(('.swift','.h','.modulemap','Package.resolved'))]
tracked+=['apps/mac/build/ffi/libtessera_ffi.a']
def freeze():return {'head':git('rev-parse','HEAD'),'status':git('status','--porcelain'),'files':{p:sha(root/p) for p in tracked}}
env=os.environ.copy()
env['CLANG_MODULE_CACHE_PATH']=str(cache/'clang-module-cache')
env['SWIFTPM_MODULECACHE_OVERRIDE']=str(cache/'swift-module-cache')
(base/'environment.json').write_text(json.dumps({k:env[k] for k in ['CLANG_MODULE_CACHE_PATH','SWIFTPM_MODULECACHE_OVERRIDE']},indent=2)+'\n')
filters=[('focused','testThrownAdmissionErrorIsVisibleAndFreshOpenSucceedsAfterReentry|testSupersededAdmissionErrorPreservesReplacementLoadingAndReadyState'),('adjacent','DevelopRecoveryCoordinatorTests|DevelopRecoveryAdmissionTests|DevelopRecoveryAdmissionBehaviorTests|DevelopRecoveryStateTests')]
for name,filt in filters:
 out=base/name
 out.mkdir(exist_ok=False)
 cmd=['swift','test','--package-path','apps/mac','--scratch-path',str(cache/'scratch'),'--cache-path',str(cache/'package-cache'),'-c','release','--jobs','2','-Xswiftc','-enable-testing','--filter',filt]
 (out/'command.txt').write_text(shlex.join(cmd)+'\n')
 before=freeze();(out/'freeze-before.json').write_text(json.dumps(before,indent=2)+'\n')
 start=time.monotonic()
 with (out/'swift-test.log').open('wb') as log:
  proc=subprocess.Popen(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  (out/'pid.txt').write_text(str(proc.pid)+'\n')
  print(f'{name} started PID {proc.pid}',flush=True)
  try:code=proc.wait(timeout=1800)
  except subprocess.TimeoutExpired:
   import signal
   os.killpg(proc.pid,signal.SIGTERM)
   code=proc.wait(timeout=30)
   (out/'watchdog.txt').write_text('1800 second timeout; SIGTERM sent to isolated process group\n')
 (out/'direct-exit.txt').write_text(str(code)+'\n')
 (out/'elapsed-seconds.txt').write_text(f'{time.monotonic()-start:.3f}\n')
 after=freeze();(out/'freeze-after.json').write_text(json.dumps(after,indent=2)+'\n')
 print(json.dumps({'attempt':name,'exit':code,'frozen_inputs':len(before['files']),'unchanged':before==after}),flush=True)
 assert before==after,'inputs changed'
 if code:break
