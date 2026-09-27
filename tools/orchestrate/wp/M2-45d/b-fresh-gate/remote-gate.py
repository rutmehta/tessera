import datetime,hashlib,io,json,os,subprocess,tarfile,time
from pathlib import Path
BASE='69bcd3e5d3d30ac334720ec6a02e1688b6f6b5cb';INITIAL='01f54a9b1e65253a68e6ae7ce13c0770ba40c1fd'
HASH='0c94c47d11d8375e07c4827ad0ae1c069981e6c5ede91786734409e166a853a6'
ROOT='01a0e327-1570-7d83-bc33-7ed882318eea'
WT=Path('/Users/rutmehta/Developer/lightroom/.worktrees/B5-16a');OUT=Path('/tmp/tessera-gainmap-b-fresh-gate');NOTE=Path('/tmp/tessera-machine-a-benchmark-owner.txt');CACHE='/Users/rutmehta/.cache/tessera-target/B5-15'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def read(cmd):return subprocess.check_output(cmd,cwd=WT,text=True).strip()
def conflict():
 for line in read(['/bin/ps','-axo','pid=,comm=']).splitlines():
  parts=line.split(None,1)
  if len(parts)<2:continue
  name=Path(parts[1]).name.lower()
  if name in ['cargo','rustc','swift','swift-frontend','xcodebuild','clang','clang++','cmake','ninja','tessera'] or 'resident_styles' in name or name.endswith('.xctest') or name.startswith('gain_map-') or name.startswith('gain_map_hosts-'):raise RuntimeError('Competing build/test: '+line)
conflict()
assert not read(['git','status','--porcelain'])
assert read(['git','rev-parse','HEAD'])==INITIAL
assert read(['git','cat-file','-t',BASE])=='commit'
archive=Path('/tmp/tessera-gainmap-approved-snapshot.tar').read_bytes();assert hashlib.sha256(archive).hexdigest()==HASH
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
 data={m.name:tar.extractfile(m).read() for m in tar.getmembers() if m.isfile()}
source=json.loads(data['manifest.json']);assert source['base']==BASE
assert set(data)=={'tracked.patch','manifest.json',*('new/'+n for n in source['untracked'])}
assert hashlib.sha256(data['tracked.patch']).hexdigest()==source['tracked_patch_sha256']
OUT.mkdir(exist_ok=False)
(OUT/'snapshot.tar').write_bytes(archive);(OUT/'source-manifest.json').write_bytes(data['manifest.json']);(OUT/'tracked.patch').write_bytes(data['tracked.patch'])
if NOTE.exists():
 previous=json.loads(NOTE.read_text());assert previous['status'].startswith('completed:')
 try:os.kill(previous['pid'],0)
 except ProcessLookupError:pass
 else:raise RuntimeError('Previous owner PID still alive')
 NOTE.rename(OUT/'previous-owner.json')
owner={'root_chat_id':ROOT,'pid':os.getpid(),'start_utc':now(),'checkout':str(WT),'base':BASE,'snapshot_sha256':HASH,'status':'preparing','heavy_slot':'Machine B one compile/test job','command':'core gain_map compile then 5 tests; if pass, CLI/FFI/MCP gain_map_hosts','preserve':'leave dirty snapshot checkout intact after completion'}
fd=os.open(NOTE,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
with os.fdopen(fd,'w') as f:json.dump(owner,f,indent=2)
env=os.environ.copy();env.update(PATH='/Users/rutmehta/.cargo/bin:/opt/homebrew/bin:'+env.get('PATH',''),CARGO_TARGET_DIR=CACHE,CARGO_BUILD_JOBS='2',MACOSX_DEPLOYMENT_TARGET='15.0')
manifest={'started_utc':now(),'root_chat_id':ROOT,'base':BASE,'snapshot_sha256':HASH,'original_branch':read(['git','rev-parse','wp/B5-16a']),'environment':{k:env[k] for k in ['PATH','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','MACOSX_DEPLOYMENT_TARGET']},'runs':[]}
def persist(): (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def own(s):owner.update(status=s,updated_utc=now());NOTE.write_text(json.dumps(owner,indent=2)+'\n')
def snap(name):
 with (OUT/(name+'-host.txt')).open('w') as f:
  for cmd in [['/usr/bin/sw_vers'],['/usr/bin/uptime'],['/bin/ps','-axo','pid,ppid,etime,pcpu,comm'],['/usr/bin/pmset','-g','therm']]:
   f.write('\n'+repr(cmd)+'\n');f.flush();subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,timeout=15)
def run(name,cmd):
 conflict();own(name);snap(name);start=now();clock=time.monotonic();print('START',name,start,flush=True)
 with (OUT/(name+'.log')).open('w') as f:
  f.write('UTC '+start+'\nCWD '+str(WT)+'\nCOMMAND '+repr(cmd)+'\nENV '+json.dumps(manifest['environment'])+'\n');f.flush();r=subprocess.run(cmd,cwd=WT,env=env,stdout=f,stderr=subprocess.STDOUT)
 entry=dict(name=name,command=cmd,start_utc=start,end_utc=now(),seconds=time.monotonic()-clock,exit_code=r.returncode);manifest['runs'].append(entry);persist();print('END',name,'exit',r.returncode,flush=True);return r.returncode
def verify():
 for name,digest in source['source_sha256'].items():assert hashlib.sha256((WT/name).read_bytes()).hexdigest()==digest,name
 actual=set(subprocess.check_output(['git','diff','--name-only'],cwd=WT,text=True).splitlines()+subprocess.check_output(['git','ls-files','--others','--exclude-standard'],cwd=WT,text=True).splitlines())
 assert actual==set(source['source_sha256']),(actual,set(source['source_sha256']))
 assert read(['git','rev-parse','HEAD'])==BASE
 assert read(['git','rev-parse','wp/B5-16a'])==manifest['original_branch']
try:
 persist()
 subprocess.run(['git','checkout','--detach',BASE],cwd=WT,check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 subprocess.run(['git','apply','--check',str(OUT/'tracked.patch')],cwd=WT,check=True)
 for n in source['untracked']:assert not (WT/n).exists(),n
 subprocess.run(['git','apply',str(OUT/'tracked.patch')],cwd=WT,check=True)
 for n in source['untracked']:
  p=WT/n;p.parent.mkdir(parents=True,exist_ok=True)
  with p.open('xb') as f:f.write(data['new/'+n])
 verify();manifest['prebuild_source_verified']=True;manifest['initial_status']=read(['git','status','--short']);persist()
 if run('core-compile',['cargo','test','--locked','-p','export','--release','--test','gain_map','--no-run']):raise RuntimeError('Core compile failed; no test run')
 if run('core-tests',['cargo','test','--locked','-p','export','--release','--test','gain_map','--','--nocapture','--test-threads=1']):raise RuntimeError('Core test failure retained; no host runs')
 for package in ['tessera-cli','tessera-ffi','tessera-mcp']:
  if run(package+'-hosts',['cargo','test','--locked','-p',package,'--release','--test','gain_map_hosts','--','--nocapture','--test-threads=1']):raise RuntimeError(package+' host failure retained')
 verify();manifest['final_status']=read(['git','status','--short']);manifest['completed_utc']=now();manifest['outcome']='all core and host tests passed';persist();own('completed: '+manifest['outcome']);print('COMPLETE',manifest['outcome'],flush=True)
except Exception as e:
 manifest['failure']=str(e);manifest['completed_utc']=now();manifest['final_status']=read(['git','status','--short']);persist();own('completed: failure retained - '+str(e));print('STOPPED',str(e),flush=True);raise
