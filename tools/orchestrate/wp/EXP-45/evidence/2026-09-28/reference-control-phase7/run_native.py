from pathlib import Path
import subprocess,json,hashlib
ROOT=Path(__file__).resolve().parents[1];OUT=Path(__file__).resolve().parent;N=OUT/'native';N.mkdir(exist_ok=True)
A=Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg')
G=ROOT/'bright-white/reference-16-bright255-explicit.jpg'
files=[G,OUT/'reference-16-split-explicit.jpg',A]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
provenance={'files':[{'path':str(p),'sha256':sha(p)} for p in files],'binaries':[{'path':str(ROOT/n),'sha256':sha(ROOT/n)} for n in ('imageio-probe','coreimage-probe')]}
(N/'INPUTS.json').write_text(json.dumps(provenance,indent=2)+'\n')
for probe in ('imageio','coreimage'):
 cmd=[str(ROOT/f'{probe}-probe')]+([str(N/'coreimage')] if probe=='coreimage' else [])+[str(p) for p in files]
 run=subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 (N/f'{probe}.command.json').write_text(json.dumps(cmd,indent=2)+'\n')
 (N/f'{probe}.stdout').write_bytes(run.stdout);(N/f'{probe}.stderr').write_bytes(run.stderr);(N/f'{probe}.exit').write_text(str(run.returncode)+'\n')
 print(probe,'exit',run.returncode,'stdout_bytes',len(run.stdout),'stderr_bytes',len(run.stderr))
 assert run.returncode==0,(probe,run.stderr.decode(errors='replace'))
 if probe=='imageio':
  idx=run.stdout.find(b'[\n');assert idx>=0
  (N/'imageio-prefix.txt').write_bytes(run.stdout[:idx]);data=json.loads(run.stdout[idx:]);(N/'imageio-parsed.json').write_text(json.dumps(data,indent=2)+'\n')
 else:data=json.loads(run.stdout);(N/'coreimage-parsed.json').write_text(json.dumps(data,indent=2)+'\n')
 print(probe,'records',len(data))
