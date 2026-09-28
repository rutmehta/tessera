import subprocess,pathlib,json,hashlib,shutil
root=pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera');out=pathlib.Path('/Volumes/betterSSD/tessera-validation/smart-previews/gpu-integration');head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip();paths=subprocess.check_output(['git','diff-tree','--no-commit-id','--name-only','-r',head],cwd=root).decode().splitlines();assert len(paths)==14
h=lambda data:hashlib.sha256(data).hexdigest()
files={p:h((root/p).read_bytes()) for p in paths};assert all(h(subprocess.check_output(['git','show',head+':'+p],cwd=root))==d for p,d in files.items())
comparisons={}
for label,file in [('28','28-default-engine-viewport/before.json'),('31','31-default-ffi-all.sources-before.json'),('32','32-default-public-workflow.sources-before.json'),('33','33-default-strict.sources-before.json'),('36','36-final-formatted-gpu.sources-before.json')]:
 prior=json.loads((out/file).read_text())['files'];comparisons[label]={'equal':[p for p,d in files.items() if prior.get(p)==d],'changed':[p for p,d in files.items() if prior.get(p)!=d]}
 assert comparisons[label]['changed']==([] if label=='36' else ['crates/pipeline-gpu/tests/smart_preview.rs'])
(out/'CANDIDATE-SOURCES.json').write_text(json.dumps({'commit':head,'base':subprocess.check_output(['git','rev-parse',head+'^'],cwd=root).decode().strip(),'files':files,'prior_gate_comparison':comparisons,'only_final_delta':'rustfmt layout of is_multiple_of test fixture, refreshed by36; product bytes identical'},indent=2))
for p in paths:
 dest=out/'candidate-source'/p;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(root/p,dest)
(out/'candidate.patch').write_bytes(subprocess.check_output(['git','show','--format=fuller','--binary',head],cwd=root))
print(head,len(files),'sources captured and Git-verified')
