import hashlib, json, os, pathlib, subprocess, sys
root=pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
out=pathlib.Path('/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor')
label=sys.argv[1]
cmd=sys.argv[2:]
tracked=subprocess.run(['git','diff','--binary','HEAD'],cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
status=subprocess.run(['git','status','--short'],cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
paths=['crates/engine-api/src/lib.rs','crates/engine-api/src/pinned_raw.rs','crates/engine-api/tests/pinned_raw.rs','crates/engine-api/CONTRACTS.md']
def source_hashes(): return {p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in paths}
def tracked_tree_hash():
 files=subprocess.run(['git','ls-files','-z'],cwd=root,stdout=subprocess.PIPE,check=True).stdout.split(b'\0')
 h=hashlib.sha256()
 for item in sorted(x for x in files if x):
  rel=item.decode(); h.update(item); h.update(hashlib.sha256((root/rel).read_bytes()).digest())
 return h.hexdigest()
freeze={'label':label,'source_hashes':source_hashes(),'tracked_tree_hash':tracked_tree_hash(),'head':subprocess.run(['git','rev-parse','HEAD'],cwd=root,text=True,stdout=subprocess.PIPE).stdout.strip(),'status_sha256':hashlib.sha256(status.stdout).hexdigest(),'diff_sha256':hashlib.sha256(tracked.stdout).hexdigest(),'status':status.stdout.decode()}
(out/f'{label}-before.json').write_text(json.dumps(freeze,indent=2)+'\n')
env=os.environ.copy(); env['CARGO_TARGET_DIR']='/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated'
proc=subprocess.run(cmd,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(out/f'{label}.log').write_bytes(proc.stdout)
(out/f'{label}-returncode.txt').write_text(str(proc.returncode)+'\n')
status2=subprocess.run(['git','status','--short'],cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
diff2=subprocess.run(['git','diff','--binary','HEAD'],cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
freeze2={'label':label,'source_hashes':source_hashes(),'tracked_tree_hash':tracked_tree_hash(),'head':subprocess.run(['git','rev-parse','HEAD'],cwd=root,text=True,stdout=subprocess.PIPE).stdout.strip(),'status_sha256':hashlib.sha256(status2.stdout).hexdigest(),'diff_sha256':hashlib.sha256(diff2.stdout).hexdigest(),'status':status2.stdout.decode()}
(out/f'{label}-after.json').write_text(json.dumps(freeze2,indent=2)+'\n')
print(f'{label}: child returncode={proc.returncode}; log={out}/{label}.log')
sys.exit(0)
