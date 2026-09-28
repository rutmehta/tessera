#!/usr/bin/env python3
"""Run ONLY after exclusive lane approval and candidate+hook review/application."""
import argparse, hashlib, json, os, pathlib, subprocess, sys
p=argparse.ArgumentParser();p.add_argument('--execute',action='store_true');p.add_argument('--checkout',required=True);p.add_argument('--fixture',required=True);p.add_argument('--out',required=True);p.add_argument('--edr',action='store_true');a=p.parse_args()
if not a.execute: sys.exit('Source-only harness: pass --execute only after explicit runtime lane approval.')
root=pathlib.Path(a.checkout);out=pathlib.Path(a.out);out.mkdir(parents=True,exist_ok=False)
fixture=pathlib.Path(a.fixture)
def digest(path):
 with open(path,'rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()
def freeze():
 names=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard','crates','Cargo.toml','Cargo.lock'],cwd=root).decode().split('\0')
 return {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip(),'files':{n:digest(root/n) for n in names if n and (root/n).is_file()},'fixture_sha256':digest(fixture)}
before=freeze();(out/'before.json').write_text(json.dumps(before,indent=2))
# Two fresh processes per forced route; few samples deliberately not a p95 claim.
jobs=[(route,'sdr',repeat) for repeat in range(2) for route in ('original-gpu','proxy-cpu','proxy-gpu')]
jobs += [(route,'sdr',0) for route in ('original-auto','proxy-auto')]
if a.edr: jobs += [(route,'edr',0) for route in ('original-gpu','proxy-cpu','proxy-gpu')]
for route,fmt,repeat in jobs:
 dest=out/f'{route}-{fmt}-{repeat}';dest.mkdir()
 env=dict(os.environ);env.update(CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated',CARGO_BUILD_JOBS='2',MACOSX_DEPLOYMENT_TARGET='15.0',TESSERA_SMART_PREVIEW_RAW=str(fixture),TESSERA_QUALIFY_ROUTE=route,TESSERA_QUALIFY_FORMAT=fmt,TESSERA_QUALIFY_OUT=str(dest),TESSERA_SMART_PREVIEW_GPU='0' if route=='proxy-cpu' else '1')
 env['TESSERA_RENDER_BACKEND']='gpu' if route.endswith('-gpu') else 'cpu' if route=='proxy-cpu' else ''
 cmd=['cargo','test','-p','tessera-ffi','--lib','--release','engine_iosurface_matched_viewport_qualification','--','--ignored','--nocapture','--test-threads=1']
 (dest/'command.json').write_text(json.dumps({'argv':cmd,'env':{k:v for k,v in env.items() if k.startswith(('CARGO_','TESSERA_','MACOSX_'))}},indent=2))
 with open(dest/'run.log','w') as log: result=subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 (dest/'exit').write_text(str(result.returncode)+'\n')
 after=freeze();(dest/'sources-after.json').write_text(json.dumps(after,indent=2));(out/'after.json').write_text(json.dumps(after,indent=2));(dest/'freeze.json').write_text(json.dumps({'equal':before==after}))
 assert result.returncode==0, f'{route} failed; inspect {dest}/run.log'
 data=json.loads((dest/'results.json').read_text());assert len(data['rows'])==24
 assert all(r['route']==route for r in data['rows'])
 assert before==freeze(),'source or original fixture changed'
(out/'after.json').write_text(json.dumps(freeze(),indent=2))
