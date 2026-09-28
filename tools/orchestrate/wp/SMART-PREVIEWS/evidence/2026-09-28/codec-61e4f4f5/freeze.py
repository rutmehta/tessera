import subprocess,hashlib,json,pathlib,sys
root=pathlib.Path.cwd()
files=subprocess.check_output(['git','ls-files','-z']).decode().split('\0')
result={}
for name in files:
 if not name:continue
 p=root/name
 if p.is_file():result[name]=hashlib.sha256(p.read_bytes()).hexdigest()
pathlib.Path(sys.argv[1]).write_text(json.dumps({'head':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'files':result},sort_keys=True,indent=2))
