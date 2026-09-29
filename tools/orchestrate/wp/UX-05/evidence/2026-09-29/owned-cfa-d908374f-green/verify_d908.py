import json,hashlib,subprocess,pathlib,os,stat,re,glob
ROOT='/Users/rutmehta/.codex/worktrees/raw-render-admission/tessera'
E=pathlib.Path('/Volumes/betterSSD/tessera-validation/owned-captured-cfa/d908374f/retry-v2')
C='d908374fd530d925e440bb21df209e60e694209b'
def g(*a,**k):return subprocess.check_output(['git','-C',ROOT,*a],**k)
def sha(b):return hashlib.sha256(b).hexdigest()
def fsha(p):
  with open(p,'rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
out={}
out['head']=g('rev-parse','HEAD',text=True).strip()
out['status_empty']=g('status','--porcelain',text=True)==''
# tree at C
tree={}
for line in g('ls-tree','-r','-z',C).split(b'\0'):
  if not line:continue
  meta,path=line.split(b'\t',1);mode,typ,obj=meta.split()
  tree[path.decode()]=(mode.decode(),typ.decode(),obj.decode())
# blob content sha256 via cat-file --batch
blobs=[v[2] for v in tree.values() if v[1]=='blob']
p=subprocess.run(['git','-C',ROOT,'cat-file','--batch'],input=('\n'.join(blobs)+'\n').encode(),capture_output=True,check=True).stdout
bsha={};i=0
while i<len(p):
  nl=p.index(b'\n',i);h,t,sz=p[i:nl].split();sz=int(sz);bsha[h.decode()]=sha(p[nl+1:nl+1+sz]);i=nl+1+sz+1
gitsha={k:bsha[v[2]] for k,v in tree.items() if v[1]=='blob'}
symlinks=[k for k,v in tree.items() if v[0]=='120000']
# working files
work={k:fsha(os.path.join(ROOT,k)) for k in tree if os.path.isfile(os.path.join(ROOT,k))}
# git show spot check for raw-decode + sample via git show
show_checked=0
rd=[k for k in tree if k.startswith('crates/raw-decode/')]
import random;random.seed(908)
sample=sorted(set(rd)|set(random.sample(sorted(k for k in tree if k not in symlinks),250)))
show_mismatch=[]
for k in sample:
  if k in symlinks:continue
  b=g('show',f'{C}:{k}')
  show_checked+=1
  if sha(b)!=gitsha[k] or work.get(k)!=sha(b):show_mismatch.append(k)
out['git_show_checked']=show_checked;out['git_show_raw_decode_files']=len(rd);out['git_show_mismatch']=show_mismatch
out['symlinks_in_tree']=symlinks
out['work_vs_blob_mismatch']=[k for k in gitsha if k not in symlinks and work.get(k)!=gitsha[k]]
runner=fsha(E/'run.py');oracle=fsha(E/'required-tests.json');fx=fsha(E/'fixture-env.json');apio=gitsha['crates/raw-decode/tests/owned_api/expectations.json']
fenv=json.load(open(E/'fixture-env.json'))
# fixture actual hashes now
fxdir=pathlib.Path(fenv['directory'])
fxnames={q.name for q in fxdir.iterdir() if q.is_file() and q.suffix.lower() in ['.arw','.cr3','.nef','.raf','.dng']}
fxall=sorted(q.name for q in fxdir.iterdir())
fxnow={n:{'path':v['path'],'bytes':os.path.getsize(v['path']),'sha256':fsha(v['path'])} for n,v in fenv['files'].items()}
auth=json.load(open(fenv['authority']))
out['fixture_dir_listing']=fxall
out['fixture_names_match']=fxnames==set(fenv['files'])
out['fixture_now_matches_env']=fxnow==fenv['files']
out['fixture_authority_raw']=auth
def check_freeze(f,label):
  r={}
  r['head_ok']=f['head']==C;r['status_empty']=f['status']=='';src=f['source']
  r['entries']=len(src)
  r['keys_equal_tree_files']=set(src)==set(k for k in tree if os.path.isfile(os.path.join(ROOT,k)))
  r['extra_keys']=sorted(set(src)-set(tree))[:20]
  r['mismatch_vs_blob']=[k for k,v in src.items() if k in gitsha and k not in symlinks and gitsha[k]!=v]
  r['symlink_entries']=[k for k in src if k in symlinks]
  r['runner_ok']=f['runner']==runner;r['oracle_ok']=f['oracle']==oracle;r['api_oracle_ok']=f['api_oracle']==apio;r['fixture_manifest_ok']=f['fixture_manifest']==fx
  r['fixtures_ok']=f['fixtures']==fenv['files']
  r['ok']=r['head_ok'] and r['status_empty'] and r['keys_equal_tree_files'] and not r['mismatch_vs_blob'] and r['runner_ok'] and r['oracle_ok'] and r['api_oracle_ok'] and r['fixture_manifest_ok'] and r['fixtures_ok']
  return r
art=json.load(open(E/'artifacts.json'))
phases={}
total_entries=0
for ph in sorted(x.name for x in E.iterdir() if x.is_dir() and x.name[0].isdigit()):
  d=E/ph;r={}
  b=json.load(open(d/'before.json'));a=json.load(open(d/'after.json'))
  r['before']=check_freeze(b,'b');r['after_equal_before']=a==b;total_entries+=r['before']['entries']
  r['freeze_json']=json.load(open(d/'freeze.json'))
  cmds=[d/'command'] if (d/'command').is_dir() else sorted(x for x in d.iterdir() if x.is_dir())
  r['commands']={}
  for c in cmds:
    cr={}
    ib=json.load(open(c/'inputs-before.json'));ia=json.load(open(c/'inputs-after.json'))
    cr['exit']=(c/'exit').read_text().strip()
    cr['inputs_equal']=ib==ia
    cr['source_freeze']=check_freeze(ib['source'],'i');total_entries+=cr['source_freeze']['entries']
    cr['source_equals_phase_before']=ib['source']==b
    cr['deps']=None if ib['dependencies'] is None else ib['dependencies']==art['dependency_context']
    cr['launch_error']=(c/'launch-error.json').exists()
    cr['argv']=json.load(open(c/'command.json'))['argv']
    r['commands'][c.name]=cr
  r['oracle_error']=(d/'oracle-error.json').exists()
  if (d/'artifacts-before.json').exists():
    r['artifacts_before_eq']=json.load(open(d/'artifacts-before.json'))==art
    r['artifacts_after_eq']=json.load(open(d/'artifacts-after.json'))==art
  phases[ph]=r
out['phases']=phases;out['total_freeze_entries_compared']=total_entries
out['tree_blob_count']=len(gitsha)
# retained
ret={}
for k in ['unit','integration']:
  pth=art[k]['path'];st=os.stat(pth)
  ret[k]={'sha_ok':fsha(pth)==art[k]['sha256'],'mode':oct(stat.S_IMODE(st.st_mode)),'exec':bool(st.st_mode&0o111),'source_exists':os.path.exists(art[k]['source_path'])}
  if ret[k]['source_exists']:
    s2=os.stat(art[k]['source_path']);ret[k]['source_mode']=oct(stat.S_IMODE(s2.st_mode));ret[k]['source_sha_ok']=fsha(art[k]['source_path'])==art[k]['sha256']
for k,v in art['libs'].items():ret['lib_'+k]=fsha(v['path'])==v['sha256']
dc=art['dependency_context'];ret['dep_context_count']=len(dc);ret['dep_context_now_mismatch']=[n for n,h in dc.items() if not os.path.isfile(n) or fsha(n)!=h]
out['retained']=ret
json.dump(out,open('/private/tmp/claude-501/-Users-rutmehta-Developer-tessera/9716362c-8ab6-410a-9e27-175afd96366b/scratchpad/raw-d908-raw-check.json','w'),indent=1)
