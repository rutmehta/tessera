import datetime,json,os,subprocess,sys,time
from pathlib import Path
SHA='01f54a9b1e65253a68e6ae7ce13c0770ba40c1fd'
ROOT='01a0e327-1570-7d83-bc33-7ed882318eea'
WT=Path('/Users/rutmehta/Developer/lightroom/.worktrees/B5-16a')
CACHE='/Users/rutmehta/.cache/tessera-target/B5-15'
OUT=Path('/tmp/tessera-m531-b-cold-diagnostic')
NOTE=Path('/tmp/tessera-machine-a-benchmark-owner.txt')
TESTS=['twenty_mp_five_styles_1368x912_l1_timing','twenty_mp_five_styles_1368x912_l1_unique_ids_timing']
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def read(cmd): return subprocess.check_output(cmd,text=True,cwd=WT).strip()
def conflicts():
 found=[]
 for line in read(['/bin/ps','-axo','pid=,ppid=,etime=,%cpu=,comm=']).splitlines():
  parts=line.split(None,4)
  if len(parts)!=5: continue
  name=Path(parts[4]).name.lower()
  if name in ['cargo','rustc','swift','swift-frontend','xcodebuild','clang','clang++','cmake','ninja'] or 'resident_styles' in name or name=='tessera' or name.endswith('.xctest'): found.append(line)
 return found
if conflicts(): raise SystemExit('Concurrent build/benchmark detected; ownership not acquired')
if read(['git','status','--porcelain']): raise SystemExit('Checkout is not clean')
if read(['git','rev-parse','HEAD'])!='441da3e3f83f46ee04536fa8b6248c2519d6d619': raise SystemExit('Unexpected initial checkout')
branch_before=read(['git','rev-parse','wp/B5-16a'])
owner={'root_chat_id':ROOT,'start_utc':now(),'pid':os.getpid(),'parent_pid':os.getppid(),'candidate':SHA,'checkout':str(WT),'heavy_slot':'Machine B: one Cargo/build/GPU benchmark at a time','command':'compile resident_styles_large --no-run; one original + one unique-ID passive diagnostic, not acceptance','status':'preparing','target_cache':CACHE,'condition':'Existing background apps and CPU yes process left untouched; capture load and thermal, never waive a failed result.'}
OUT.mkdir(exist_ok=True)
if (OUT/'manifest.json').exists(): raise SystemExit('Existing diagnostic manifest')
if NOTE.exists():
 previous=json.loads(NOTE.read_text())
 if not previous.get('status','').startswith('completed:'): raise SystemExit('Prior owner not completed')
 try: os.kill(previous['pid'],0)
 except ProcessLookupError: pass
 else: raise SystemExit('Prior owner PID still exists')
 NOTE.rename(OUT/'previous-ownership-note.json')
fd=os.open(NOTE,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
with os.fdopen(fd,'w') as f:f.write(json.dumps(owner,indent=2)+'\n')
OUT.mkdir(exist_ok=True)
if (OUT/'manifest.json').exists(): raise SystemExit('Existing evidence manifest; do not overwrite')
env=os.environ.copy();env.update(PATH='/Users/rutmehta/.cargo/bin:'+env.get('PATH',''),CARGO_TARGET_DIR=CACHE,CARGO_BUILD_JOBS='2',MACOSX_DEPLOYMENT_TARGET='15.0',TESSERA_COLD_DIAGNOSTIC='1')
manifest={'root_chat_id':ROOT,'started_utc':now(),'candidate':SHA,'checkout':str(WT),'branch_before':branch_before,'environment':{k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_BUILD_JOBS','MACOSX_DEPLOYMENT_TARGET','TESSERA_COLD_DIAGNOSTIC']},'runs':[],'source':'Machine A direct authenticated SSH execution; not B chat receipt'}
def persist(): (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def ownership(status):owner.update(status=status,updated_utc=now());NOTE.write_text(json.dumps(owner,indent=2)+'\n')
def snapshot(name):
 with (OUT/(name+'-host.txt')).open('w') as f:
  f.write('UTC '+now()+'\n')
  for cmd in [['/usr/bin/uptime'],['/bin/ps','-axo','pid,ppid,etime,pcpu,comm'],['/usr/bin/pmset','-g','therm']]:
   f.write('\nCOMMAND '+repr(cmd)+'\n');f.flush()
   try:subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,timeout=15)
   except subprocess.TimeoutExpired:f.write('SNAPSHOT TIMED OUT\n')
def logged(name,cmd):
 started=now();clock=time.monotonic();print('START',name,started,flush=True)
 with (OUT/(name+'.log')).open('w') as f:
  f.write('UTC '+started+'\nCWD '+str(WT)+'\nCOMMAND '+repr(cmd)+'\nENV '+json.dumps(manifest['environment'])+'\n');f.flush()
  r=subprocess.run(cmd,cwd=WT,env=env,stdout=f,stderr=subprocess.STDOUT)
 result={'name':name,'command':cmd,'start_utc':started,'end_utc':now(),'exit_code':r.returncode,'seconds':time.monotonic()-clock}
 print('END',name,'exit',r.returncode,flush=True);return result
try:
 persist()
 manifest['fetch']=logged('fetch',['git','fetch','origin','codex/m531-cold-diagnostics']);persist()
 if manifest['fetch']['exit_code']:raise RuntimeError('Fetch failed')
 if read(['git','rev-parse','FETCH_HEAD'])!=SHA:raise RuntimeError('Fetched head differs from approved SHA')
 manifest['checkout_result']=logged('checkout',['git','checkout','--detach',SHA]);persist()
 if manifest['checkout_result']['exit_code']:raise RuntimeError('Checkout failed')
 assert read(['git','rev-parse','HEAD'])==SHA
 assert read(['git','rev-parse','wp/B5-16a'])==branch_before
 assert not read(['git','status','--porcelain'])
 source=(WT/'crates/compositor/tests/resident_styles_large.rs').read_text()
 assert all(('fn '+t+'()') in source for t in TESTS)
 with (OUT/'host-toolchain.txt').open('w') as f:
  for cmd in [['/usr/bin/sw_vers'],['/usr/sbin/sysctl','machdep.cpu.brand_string','hw.model','hw.ncpu','hw.memsize'],['/Users/rutmehta/.cargo/bin/rustc','--version'],['/Users/rutmehta/.cargo/bin/cargo','--version'],['/usr/bin/xcode-select','-p'],['/bin/df','-h',str(WT),CACHE]]:
   f.write('COMMAND '+repr(cmd)+'\n');f.flush();subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT)
 if conflicts(): raise RuntimeError('Concurrent build detected before compile')
 ownership('compiling');snapshot('compile')
 manifest['compile']=logged('compile',['cargo','test','--locked','-p','compositor','--release','--test','resident_styles_large','--no-run']);persist()
 if manifest['compile']['exit_code']:raise RuntimeError('Compilation failed; no samples executed')
 for test in TESTS:
  for n in [1]:
   if conflicts():raise RuntimeError('New competing build/test detected; stop instead of measuring overlap')
   name=('unique' if 'unique_ids' in test else 'original')+'-'+str(n)
   ownership('timing '+name);snapshot(name)
   cmd=['cargo','test','--locked','-p','compositor','--release','--test','resident_styles_large',test,'--','--exact','--ignored','--nocapture','--test-threads=1']
   manifest['runs'].append(logged(name,cmd));persist()
 manifest['final_head']=read(['git','rev-parse','HEAD'])
 manifest['branch_after']=read(['git','rev-parse','wp/B5-16a'])
 manifest['final_status']=read(['git','status','--porcelain'])
 manifest['completed_utc']=now();manifest['outcome']='passed' if all(r['exit_code']==0 for r in manifest['runs']) else 'timing failures retained'
 ownership('completed: '+manifest['outcome']);persist()
 print('COMPLETE',manifest['outcome'],json.dumps([r['exit_code'] for r in manifest['runs']]),flush=True)
except Exception as e:
 manifest['error']=str(e);manifest['completed_utc']=now();persist();ownership('stopped: '+str(e));print('STOPPED',str(e),flush=True);raise
