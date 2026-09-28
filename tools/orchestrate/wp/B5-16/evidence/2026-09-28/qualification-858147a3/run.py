#!/usr/bin/env python3
"""No execution without explicit runtime-lane authorization and --execute."""
import argparse,hashlib,json,os,pathlib,subprocess,time
ROOT=pathlib.Path('/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera')
OUT=pathlib.Path(__file__).resolve().parent
SCRATCH=pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-cd0b850d/swift-build')
FIXTURE=pathlib.Path('/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW')
ENV={'CARGO_TARGET_DIR':'/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated','CARGO_BUILD_JOBS':'2','RAYON_NUM_THREADS':'2','MACOSX_DEPLOYMENT_TARGET':'15.0'}
def cargo(*args):return ['cargo',*args,'--','--test-threads=1']
def swift(filter=None):return ['swift','test','--package-path','apps/mac','--scratch-path',str(SCRATCH),'-c','release','--jobs','2']+(['--filter',filter] if filter else [])
GATES={
 '01-models-analysis':swift('ThemeLintTests|DocumentHistoryHeightControlTests|DocumentInspectorTabsTests|DocumentKeyRoutingTests|KeyFocusTests|DocumentAdjustmentAnalysisTests|DocumentAdjustmentJSONTests'),
 '01b-keyboard-alone':swift('DocumentKeyRoutingTests|KeyFocusTests')+['--skip-build'],
 '02-layout':swift('ShellLayoutTests'),
 '03-adjacent':swift('DocumentSaveDestinationCommitTests|DocumentSaveSettlementTests|DocumentSavePresenterTests|DocumentSaveSheetAttachmentTests|DocumentLoadSettlementTests|DocumentLoadStatusTests|DocumentOutlineLifecycleTests|EngineDocumentBackendTests|DocumentTransformTests'),
 '04-full':swift(),
 '05-strict':['swift','build','--package-path','apps/mac','--scratch-path',str(SCRATCH),'-c','release','--jobs','2','--product','Tessera','-Xswiftc','-strict-concurrency=complete','-Xswiftc','-warnings-as-errors'],
}
TEST_GATES=set(GATES)-{'05-strict'}
REQUIRED={
 '01-models-analysis': [('ThemeLintTests','testViewsUseThemeTokensOnly')]+[('DocumentHistoryHeightControlTests',n) for n in ['testGlobalStateRestoresExistingOwnersAndPreferenceValues','testNativeAXPressActionsChangeHeightAndExposeCurrentValue','testActionsUseDisplayedClampButLayoutDoesNotRewriteSavedRequest','testFocusedButtonsOwnSpaceAndReturnAndSendTheirActions','testConfigureRefreshesCallbackWithoutPublishingAndTeardownStopsActions','testActualInspectorAXActionWritesExistingPreferenceAndRestoresOnRecreation']]+[('DocumentInspectorTabsTests',n) for n in ['testTabsAndShortcuts','testBudgetFitsTheSmallestColumn','testHistoryHeightIsClampedToItsMinimumAndTheTabMinimum','testOpacityAndFillShareARowFrom300Points','testTabStripShowsAtMostThreeIncludingTheCurrentOne']]+[('DocumentAdjustmentAnalysisTests',n) for n in ['testLegacyMatchColorNeutralizeReopenDisableUndoAndSave','testLegacyMatchColorMissingSourceDoesNotPretendToDisable','testModernMatchColorToggleKeepsFrozenStatisticsWithoutReadingPixels']],
 '02-layout':[('ShellLayoutTests',n) for n in ['testShellContainedAtEverySizeStateAndAppearance','testDocumentInspectorEveryTabAndHistoryStateAtEverySize','testManyDocumentTabsStayCapped','testInspectorTabShortcuts']],
 '04-full':[('SmartPreviewNativeWorkflowTests',n) for n in ['testActualCachedThumbnailAfterOriginalDisconnect','testActualSwiftBridgeOfflineLibraryRenderSaveReopenAndReconnect']],
}
REQUIRED['04-full'] += json.loads((OUT/'required-adjacent-full.json').read_text())
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
 baseline=json.loads((OUT/'prepared-source-and-artifacts.json').read_text())
 if any(before[k]!=baseline[k] for k in ['head','source','ignored_ffi','fixture']):raise SystemExit('PRE-RUN baseline mismatch; stop for explicit reviewed baseline refresh')
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
 if changed or before['head']!=after['head']:raise SystemExit('UNEXPECTED source drift; preserve evidence and inspect')
 if before['fixture']!=after['fixture']:raise SystemExit('FIXTURE CHANGED; preserve evidence and stop')
 if before['ignored_ffi']!=after['ignored_ffi']:raise SystemExit('UNEXPECTED ignored FFI artifact drift; preserve evidence and stop')
 if r.returncode==0 and a.gate in TEST_GATES:
  import re
  log=(dest/'run.log').read_text()
  if not re.search(r'Executed [1-9][0-9]* tests?',log):raise SystemExit('No positive XCTest count')
  matches={}
  required=REQUIRED.get(a.gate,[])
  if a.gate=='04-full':required=REQUIRED['01-models-analysis']+REQUIRED['02-layout']+required
  for cls,name in required:
   lines=[line for line in log.splitlines() if (cls+'.'+name in line or cls+' '+name+']' in line) and "' passed" in line]
   matches[cls+'.'+name]=lines
   if len(lines)!=1:raise SystemExit('Required test did not report exactly one pass: '+cls+'.'+name)
  (dest/'required-tests.json').write_text(json.dumps(matches,indent=2))
 raise SystemExit(r.returncode)
