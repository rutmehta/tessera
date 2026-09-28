import sys,os,subprocess,hashlib,json,time,pathlib
root=pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
out=pathlib.Path(__file__).parent
name=sys.argv[1]; cmd=sys.argv[2:]
def manifest():
 names=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=root).decode().split('\0')
 data={}
 for name in sorted(set(names)):
  p=root/name
  if name and p.is_file(): data[name]=hashlib.sha256(p.read_bytes()).hexdigest()
 return {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip(),'files':data}
env=os.environ.copy();env.update(CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated',MACOSX_DEPLOYMENT_TARGET='15.0',CARGO_BUILD_JOBS='2')
before=manifest();(out/(name+'-before.json')).write_text(json.dumps(before,sort_keys=True,indent=2))
start=time.time()
with (out/(name+'.log')).open('w') as log:
 result=subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
after=manifest();(out/(name+'-after.json')).write_text(json.dumps(after,sort_keys=True,indent=2))
record={'command':cmd,'cwd':str(root),'environment':{k:env[k] for k in ['CARGO_TARGET_DIR','MACOSX_DEPLOYMENT_TARGET','CARGO_BUILD_JOBS']},'exit':result.returncode,'seconds':time.time()-start,'source_equal':before==after}
(out/(name+'-result.json')).write_text(json.dumps(record,indent=2));print(json.dumps(record));sys.exit(result.returncode)
