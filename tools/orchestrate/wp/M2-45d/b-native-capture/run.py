import datetime,hashlib,io,json,os,subprocess,tarfile,time
from pathlib import Path
WT=Path('/Users/rutmehta/Developer/lightroom/.worktrees/B5-16a');OUT=Path('/tmp/tessera-gainmap-b-capture');NOTE=Path('/tmp/tessera-machine-a-benchmark-owner.txt')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def read(c):return subprocess.check_output(c,cwd=WT,text=True).strip()
def conflict():
 for l in read(['/bin/ps','-axo','pid=,comm=']).splitlines():
  n=Path(l.split(None,1)[1]).name.lower()
  if n in ['cargo','rustc','swift','swift-frontend','clang','clang++','xcodebuild','tessera'] or n.startswith('gain_map-') or n.startswith('gain_map_hosts-'):raise RuntimeError('Competing process '+l)
conflict()
source=json.loads(Path('/tmp/tessera-gainmap-b-fresh-gate/source-manifest.json').read_text())
assert read(['git','rev-parse','HEAD'])==source['base']
for n,h in source['source_sha256'].items():assert hashlib.sha256((WT/n).read_bytes()).hexdigest()==h,n
b=Path('/tmp/tessera-gainmap-capture-inputs.tar').read_bytes();assert hashlib.sha256(b).hexdigest()=='4b79b1189d7ef9c94199ba523db018395518d9ce4c31c85892c63e677a57055d'
OUT.mkdir(exist_ok=False)
with tarfile.open(fileobj=io.BytesIO(b)) as t:
 files={m.name:t.extractfile(m).read() for m in t.getmembers() if m.isfile()}
hashes=json.loads(files['inputs-sha256.json'])
for n,h in hashes.items():assert hashlib.sha256(files[n]).hexdigest()==h
for n,d in files.items():assert '/' not in n; (OUT/n).write_bytes(d)
previous=json.loads(NOTE.read_text());assert previous['status'].startswith('completed:')
try:os.kill(previous['pid'],0)
except ProcessLookupError:pass
else:raise RuntimeError('Previous owner PID alive')
NOTE.rename(OUT/'previous-owner.json')
owner={'root_chat_id':'01a0e327-1570-7d83-bc33-7ed882318eea','pid':os.getpid(),'start_utc':now(),'status':'preparing capture diagnostic','checkout':str(WT),'source_snapshot':'0c94c47d11d8375e07c4827ad0ae1c069981e6c5ede91786734409e166a853a6','command':'one canonical capture test then existing native probe, no host gates'}
fd=os.open(NOTE,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
with os.fdopen(fd,'w') as f:json.dump(owner,f,indent=2)
env=os.environ.copy();env.update(PATH='/Users/rutmehta/.cargo/bin:/opt/homebrew/bin:'+env.get('PATH',''),CARGO_TARGET_DIR='/Users/rutmehta/.cache/tessera-target/B5-15',CARGO_BUILD_JOBS='2',MACOSX_DEPLOYMENT_TARGET='15.0',TESSERA_GAINMAP_ARTIFACT_DIR=str(OUT/'artifacts'),PROBE_OPTIONS='none',PROBE_DUMP=str(OUT/'artifacts'))
manifest={'start_utc':now(),'source_manifest':source,'input_hashes':hashes,'runs':[],'environment':{k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_BUILD_JOBS','MACOSX_DEPLOYMENT_TARGET','TESSERA_GAINMAP_ARTIFACT_DIR','PROBE_OPTIONS','PROBE_DUMP']}}
def persist(): (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def run(name,cmd):
 conflict();owner.update(status=name,updated_utc=now());NOTE.write_text(json.dumps(owner,indent=2));print('START',name,flush=True);start=now();clock=time.monotonic()
 with (OUT/(name+'.log')).open('w') as f:
  f.write('COMMAND '+repr(cmd)+'\nUTC '+start+'\n');f.flush();r=subprocess.run(cmd,cwd=WT,env=env,stdout=f,stderr=subprocess.STDOUT)
 manifest['runs'].append(dict(name=name,command=cmd,exit=r.returncode,start=start,end=now(),seconds=time.monotonic()-clock));persist();print('END',name,r.returncode,flush=True);return r.returncode
try:
 with (OUT/'host.txt').open('w') as f:
  for c in [['/usr/bin/sw_vers'],['/usr/bin/uptime'],['/bin/ps','-axo','pid,ppid,etime,pcpu,comm'],['/usr/bin/pmset','-g','therm']]:subprocess.run(c,stdout=f,stderr=subprocess.STDOUT)
 subprocess.run(['git','apply','--check',str(OUT/'capture.patch')],cwd=WT,check=True)
 subprocess.run(['git','apply',str(OUT/'capture.patch')],cwd=WT,check=True)
 manifest['capture_test_sha256']=hashlib.sha256((WT/'crates/export/tests/gain_map.rs').read_bytes()).hexdigest();persist()
 run('canonical-capture',['cargo','test','--locked','-p','export','--release','--test','gain_map','iso_gain_map_reconstructs_reference_hdr_and_has_real_mpf_associations','--','--exact','--nocapture','--test-threads=1'])
 assert (OUT/'artifacts/fresh-gain-map.jpg').is_file(),'Canonical artifact not written'
 assert run('probe-compile',['/usr/bin/xcrun','clang','-fobjc-arc','-framework','Foundation','-framework','ImageIO','-framework','CoreGraphics',str(OUT/'decode-probe.m'),'-o',str(OUT/'decode-probe')])==0
 run('pixel-decode',[str(OUT/'decode-probe'),str(OUT/'artifacts/fresh-gain-map.jpg'),str(OUT/'gainmap_iso21496_1.jpg'),str(OUT/'gainmap_iso21496_1_adobe_gcontainer.jpg')])
 manifest['final_status']=read(['git','status','--short']);manifest['completed_utc']=now();persist();owner.update(status='completed: diagnostic captured; original core gate still failed',updated_utc=now());NOTE.write_text(json.dumps(owner,indent=2));print('COMPLETE',flush=True)
except Exception as e:
 manifest['failure']=str(e);persist();owner.update(status='completed: diagnostic failure retained '+str(e),updated_utc=now());NOTE.write_text(json.dumps(owner,indent=2));raise
