#!/usr/bin/env python3
"""No execution without explicit runtime-lane authorization and --execute."""
import argparse,hashlib,json,os,pathlib,subprocess,time
ROOT=pathlib.Path('/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera')
OUT=pathlib.Path(__file__).resolve().parent
SCRATCH=OUT/'swift-build'
FIXTURE=pathlib.Path('/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW')
ENV={'CARGO_TARGET_DIR':'/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated','CARGO_BUILD_JOBS':'2','RAYON_NUM_THREADS':'2','MACOSX_DEPLOYMENT_TARGET':'15.0'}
def cargo(*args):return ['cargo',*args,'--','--test-threads=1']
def swift(filter=None):return ['swift','test','--package-path','apps/mac','--scratch-path',str(SCRATCH),'-c','release','--jobs','2']+(['--filter',filter] if filter else [])
GATES={
 '01-native-commit':cargo('test','--locked','-p','tessera-ffi','--release','--lib','destination_commit_tests'),
 '02-native-session':cargo('test','--locked','-p','tessera-ffi','--release','--test','document_save_destination'),
 '03-native-roundtrip':cargo('test','--locked','-p','tessera-ffi','--release','--test','document','tessera_doc_round_trip_and_same_path_same_session'),
 '04-psd-psb':cargo('test','--locked','-p','tessera-ffi','--release','--test','document','psd_opens_with_names_and_modes_and_saves_back_unknown_keys'),
 '05-native-document':cargo('test','--locked','-p','tessera-ffi','--release','--test','document'),
 '06-native-ffi':cargo('test','--locked','-p','tessera-ffi','--release','--lib'),
 '07-native-strict':['cargo','clippy','--locked','-p','tessera-ffi','--release','--all-targets','--','-D','warnings'],
 '08-native-format':['cargo','fmt','--all','--','--check'],
 '09-regenerate':['bash','apps/mac/build-ffi.sh'],
 '10-swift-focused':swift('DocumentSaveDestinationCommitTests|DocumentSaveSettlementTests|EngineDocumentBackendTests'),
 '11-swift-adjacent':swift('DocumentSavePresenterTests|DocumentSaveSheetAttachmentTests|DocumentLoadSettlementTests|DocumentLoadStatusTests|DocumentOutlineLifecycleTests|DocumentTransformTests'),
 '12-swift-full':swift(),
 '13-swift-strict':['swift','build','--package-path','apps/mac','--scratch-path',str(SCRATCH),'-c','release','--jobs','2','--product','Tessera','-Xswiftc','-strict-concurrency=complete','-Xswiftc','-warnings-as-errors'],
}
TEST_GATES={'01-native-commit','02-native-session','03-native-roundtrip','04-psd-psb','05-native-document','06-native-ffi','10-swift-focused','11-swift-adjacent','12-swift-full'}
def gate_env(gate):return {**ENV,**({'TESSERA_SMART_PREVIEW_RAW':str(FIXTURE)} if gate in TEST_GATES else {})}
def digest(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def snapshot():
 names=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=ROOT).decode().split('\0')
 source={n:digest(ROOT/n) for n in names if n and (ROOT/n).is_file()}
 artifacts={str(p.relative_to(ROOT)):digest(p) for p in (ROOT/'apps/mac/build/ffi').glob('*') if p.is_file()}
 outputs={str(p):digest(p) for p in [SCRATCH/'release/Tessera',SCRATCH/'release/TesseraPackageTests.xctest/Contents/MacOS/TesseraPackageTests'] if p.is_file()}
 return {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip(),'source':source,'ignored_ffi':artifacts,'swift_outputs':outputs,'fixture':{'path':str(FIXTURE),'sha256':digest(FIXTURE)}}
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('gate',choices=list(GATES));p.add_argument('--attempt',required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
 cmd=GATES[a.gate]
 if not a.execute:print(json.dumps({'cwd':str(ROOT),'command':cmd,'env':gate_env(a.gate),'status':'PREPARED ONLY; requires root lane handoff'},indent=2));raise SystemExit(0)
 dest=OUT/(a.gate+'-'+a.attempt);dest.mkdir(exist_ok=False)
 before=snapshot();(dest/'before.json').write_text(json.dumps(before,indent=2))
 env=dict(os.environ)
 for key in list(env):
  if key.startswith('TESSERA_'):del env[key]
 env.update(gate_env(a.gate))
 (dest/'command.json').write_text(json.dumps({'cwd':str(ROOT),'argv':cmd,'env':{**ENV,**({'TESSERA_SMART_PREVIEW_RAW':env['TESSERA_SMART_PREVIEW_RAW']} if 'TESSERA_SMART_PREVIEW_RAW' in env else {})},'inherited_TESSERA_removed':True},indent=2))
 start=time.monotonic()
 with (dest/'run.log').open('w') as f:r=subprocess.run(cmd,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT)
 (dest/'exit').write_text(str(r.returncode)+'\n')
 after=snapshot();(dest/'after.json').write_text(json.dumps(after,indent=2))
 changed=sorted(k for k in before['source'].keys()|after['source'].keys() if before['source'].get(k)!=after['source'].get(k))
 (dest/'freeze.json').write_text(json.dumps({'source_equal':before['source']==after['source'],'head_equal':before['head']==after['head'],'source_changed_paths':changed,'elapsed_seconds':time.monotonic()-start,'ignored_ffi_equal':before['ignored_ffi']==after['ignored_ffi'],'fixture_equal':before['fixture']==after['fixture']},indent=2))
 allowed={'apps/mac/Sources/TesseraFFI/TesseraFFI.swift','apps/mac/Sources/CTesseraFFI/CTesseraFFI.h','apps/mac/Sources/CTesseraFFI/module.modulemap'} if a.gate=='09-regenerate' else set()
 if set(changed)-allowed or before['head']!=after['head']:raise SystemExit('UNEXPECTED source drift; preserve evidence and inspect')
 if before['fixture']!=after['fixture']:raise SystemExit('FIXTURE CHANGED; preserve evidence and stop')
 if a.gate!='09-regenerate' and before['ignored_ffi']!=after['ignored_ffi']:raise SystemExit('UNEXPECTED ignored FFI artifact drift; preserve evidence and stop')
 if a.gate=='12-swift-full' and r.returncode==0:
  log=(dest/'run.log').read_text()
  for name in ['testActualCachedThumbnailAfterOriginalDisconnect','testActualSwiftBridgeOfflineLibraryRenderSaveReopenAndReconnect']:
   if not any(('SmartPreviewNativeWorkflowTests.'+name in line or 'SmartPreviewNativeWorkflowTests '+name+']' in line) and "' passed" in line for line in log.splitlines()):raise SystemExit('Required real Smart Preview workflow did not report passed: '+name)
 raise SystemExit(r.returncode)
