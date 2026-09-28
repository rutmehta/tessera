import runpy,pathlib,json,subprocess,os,time
m=runpy.run_path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/run.py')
p=pathlib.Path(__file__).parent
before=m['snapshot']();(p/'before.json').write_text(json.dumps(before,indent=2))
assert before['head'].startswith('0e64a277')
old=json.loads(pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/prepared-source-and-artifacts.json').read_text())
assert old['ignored_ffi']==before['ignored_ffi'] and old['fixture']==before['fixture']
changed=[k for k in old['source'].keys()|before['source'].keys() if old['source'].get(k)!=before['source'].get(k)]
assert changed==['apps/mac/Tests/TesseraCoreTests/DocumentInspectorFocusRoutingTests.swift'],changed
cmd=m['swift']('DocumentInspectorFocusRoutingTests|DocumentHistoryHeightControlTests|DocumentKeyRoutingTests|KeyFocusTests|DocumentVectorVerifyFixesTests')
env={k:v for k,v in os.environ.items() if not k.startswith('TESSERA_')};env.update(m['ENV']);env['TESSERA_SMART_PREVIEW_RAW']=str(m['FIXTURE'])
(p/'command.json').write_text(json.dumps({'argv':cmd,'cwd':str(m['ROOT']),'env':{**m['ENV'],'TESSERA_SMART_PREVIEW_RAW':str(m['FIXTURE'])},'reused_scratch':str(m['SCRATCH']),'source_delta':changed},indent=2))
t=time.monotonic()
with (p/'run.log').open('w') as f:r=subprocess.run(cmd,cwd=m['ROOT'],env=env,stdout=f,stderr=subprocess.STDOUT)
(p/'exit').write_text(str(r.returncode)+'\n')
after=m['snapshot']();(p/'after.json').write_text(json.dumps(after,indent=2))
freeze={k+'_equal':before[k]==after[k] for k in ['head','source','ignored_ffi','fixture']};freeze['elapsed_seconds']=time.monotonic()-t
(p/'freeze.json').write_text(json.dumps(freeze,indent=2))
assert all(v for k,v in freeze.items() if k.endswith('_equal')),freeze
print(json.dumps({'exit':r.returncode,**freeze}));raise SystemExit(r.returncode)
