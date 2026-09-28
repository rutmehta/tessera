import json,subprocess,hashlib,sys
from pathlib import Path
root=Path.cwd()
m=json.loads(Path('/Volumes/betterSSD/tessera-validation/smart-previews/export-admission/metadata.json').read_text())
pkgs={p['name']:p for p in m['packages']}; todo=['export','pipeline-adobe'];seen=set();dirs=[]
while todo:
 name=todo.pop()
 if name in seen:continue
 seen.add(name);p=pkgs[name];dirs.append(str(Path(p['manifest_path']).parent.relative_to(root)))
 todo.extend(d['name'] for d in p['dependencies'] if d.get('path') and d['name'] in pkgs)
files=subprocess.check_output(['git','ls-files','-z','--',*dirs,'Cargo.toml','Cargo.lock','.cargo','rust-toolchain.toml']).decode().split('\0')
files=sorted(p for p in files if p and Path(p).is_file())
Path(sys.argv[1]).write_text(''.join(f'{hashlib.sha256(Path(p).read_bytes()).hexdigest()}  {p}\n' for p in files))
print('local packages:',','.join(sorted(seen)),'files:',len(files))
