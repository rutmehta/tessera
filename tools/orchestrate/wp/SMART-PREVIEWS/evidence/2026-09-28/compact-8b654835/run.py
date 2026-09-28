import subprocess,hashlib,json,pathlib,sys,os,time
out=pathlib.Path('/Volumes/betterSSD/tessera-validation/smart-previews/compact-tier')
root=pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
tag=sys.argv[1];command=sys.argv[2:];os.chdir(root)
def freeze():
 files=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard']).decode().split('\0')
 return {'head':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'files':{f:hashlib.sha256(pathlib.Path(f).read_bytes()).hexdigest() for f in files if f and pathlib.Path(f).is_file()}}
env=dict(os.environ);env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']='/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated'
(out/(tag+'.command.json')).write_text(json.dumps({'command':command,'CARGO_TARGET_DIR':env['CARGO_TARGET_DIR'],'CARGO_BUILD_JOBS':env['CARGO_BUILD_JOBS'],'cwd':str(root),'fixture_env':{k:v for k,v in env.items() if k.startswith('TESSERA_CODEC_')}},indent=2))
before=freeze();(out/(tag+'.sources-before.json')).write_text(json.dumps(before,sort_keys=True,indent=2))
start=time.time()
with (out/(tag+'.log')).open('w') as f: result=subprocess.run(command,env=env,stdout=f,stderr=subprocess.STDOUT)
(out/(tag+'.exit')).write_text(str(result.returncode)+'\n');after=freeze();(out/(tag+'.sources-after.json')).write_text(json.dumps(after,sort_keys=True,indent=2));(out/(tag+'.freeze.json')).write_text(json.dumps({'equal':before==after,'elapsed_seconds':time.time()-start},indent=2))
print(f'{tag}: exit={result.returncode} source_equal={before==after} elapsed={time.time()-start:.1f}s',flush=True)
print((out/(tag+'.log')).read_text()[-2300:],flush=True)
sys.exit(result.returncode)
