import sys,os,pathlib,hashlib,json,subprocess
root=pathlib.Path('/Users/rutmehta/.codex/worktrees/proxy-reopen-baseline/tessera');out=pathlib.Path('/Volumes/betterSSD/tessera-validation/smart-preview-reopen-baseline')/sys.argv[1];out.mkdir(exist_ok=False)
fixture=pathlib.Path('/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def freeze():
 paths=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=root).decode().split('\0')
 return {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip(),'files':{p:sha(root/p) for p in paths if p and (root/p).is_file()},'fixture_sha256':sha(fixture)}
before=freeze();(out/'before.json').write_text(json.dumps(before,indent=2));env={k:v for k,v in os.environ.items() if not k.startswith('TESSERA_')};env.update(CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated',CARGO_BUILD_JOBS='2',MACOSX_DEPLOYMENT_TARGET='15.0');cmd=sys.argv[2:];(out/'command.json').write_text(json.dumps({'argv':cmd,'env':{k:v for k,v in env.items() if k.startswith(('CARGO_','MACOSX_','TESSERA_'))}},indent=2))
with (out/'run.log').open('w') as f:r=subprocess.run(cmd,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT)
(out/'exit').write_text(str(r.returncode)+'\n');after=freeze();(out/'after.json').write_text(json.dumps(after,indent=2));(out/'freeze.json').write_text(json.dumps({'equal':before==after}));print(json.dumps({'gate':str(out),'exit':r.returncode,'frozen':before==after}));sys.exit(r.returncode if before==after else 99)
