import pathlib,subprocess,os,json,hashlib,time,re
ROOT=pathlib.Path('/Users/rutmehta/.codex/worktrees/raw-render-admission/tessera')
OUT=pathlib.Path(__file__).parent
ENV={'CARGO_TARGET_DIR':'/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated','CARGO_BUILD_JOBS':'2','RAYON_NUM_THREADS':'2','MACOSX_DEPLOYMENT_TARGET':'15.0'}
def digest(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def snapshot():
 names=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=ROOT).decode().split('\0')
 return {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'source':{n:digest(ROOT/n) for n in names if n and (ROOT/n).is_file()},'runner_sha256':digest(pathlib.Path(__file__)),'fixtures':'none; synthetic values only','git_status':subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True)}
env={k:v for k,v in os.environ.items() if not k.startswith('TESSERA_') and k!='RAW_DECODE_FIXTURES'};env.update(ENV)
before=snapshot();assert before['head']=='b950941ea8e79f66a47d4bf96dad23bee1d6005a' and not before['git_status']
(OUT/'baseline.json').write_text(json.dumps(before,indent=2))
def run(name,argv):
 p=OUT/name;p.mkdir(exist_ok=False)
 b=snapshot();assert b==before
 (p/'before.json').write_text(json.dumps(b,indent=2))
 (p/'command.json').write_text(json.dumps({'argv':argv,'cwd':str(ROOT),'env':ENV,'inherited_TESSERA_and_fixture_flags_removed':True},indent=2))
 start=time.monotonic()
 with (p/'run.log').open('w') as f:r=subprocess.run(argv,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT)
 (p/'exit').write_text(str(r.returncode)+'\n')
 a=snapshot();(p/'after.json').write_text(json.dumps(a,indent=2))
 (p/'freeze.json').write_text(json.dumps({'equal':a==b,'elapsed_seconds':time.monotonic()-start},indent=2))
 assert a==b
 print(name,'exit',r.returncode,flush=True)
 return r.returncode,(p/'run.log').read_text()
code,log=run('01-compile',['cargo','test','--jobs','2','-p','image-core','--release','--lib','raw_admission::tests','--no-run'])
if code:raise SystemExit(code)
paths=re.findall(r'Executable unittests src/lib.rs \(([^)]+)\)',log)
assert len(paths)==1,paths
exe=pathlib.Path(paths[0]);sha=digest(exe)
(OUT/'test-executable-before.json').write_text(json.dumps({'path':str(exe),'sha256':sha},indent=2))
code,log=run('02-focused-red',[str(exe),'raw_admission::tests','--test-threads=1','--nocapture'])
(OUT/'test-executable-after.json').write_text(json.dumps({'path':str(exe),'sha256':digest(exe),'equal':digest(exe)==sha},indent=2))
assert digest(exe)==sha
raise SystemExit(code)
