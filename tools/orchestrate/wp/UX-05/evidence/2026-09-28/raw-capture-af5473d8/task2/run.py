import subprocess, pathlib, hashlib, json, os, sys, datetime
root=pathlib.Path("/Users/rutmehta/.codex/worktrees/export-integration/tessera")
out=pathlib.Path(__file__).parent
name=sys.argv[1]; command=sys.argv[2:]
def freeze():
    names=set(x for x in subprocess.check_output(["git","ls-files"],cwd=root,text=True).splitlines())
    names.update(x for x in subprocess.check_output(["git","ls-files","--others","--exclude-standard"],cwd=root,text=True).splitlines())
    return {"head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),"sources":{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sorted(names) if (root/p).is_file()},"fixture":"synthetic test files only; no user RAW read"}
env=os.environ.copy();env.update(CARGO_TARGET_DIR="/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated",MACOSX_DEPLOYMENT_TARGET="15.0",CARGO_BUILD_JOBS="2")
(out/(name+"-before.json")).write_text(json.dumps(freeze(),indent=2))
(out/(name+"-command.json")).write_text(json.dumps({"command":command,"env":{k:env[k] for k in ["CARGO_TARGET_DIR","MACOSX_DEPLOYMENT_TARGET","CARGO_BUILD_JOBS"]},"start":datetime.datetime.now(datetime.timezone.utc).isoformat()},indent=2))
with (out/(name+".log")).open("w") as log:
    result=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
(out/(name+".exit")).write_text(str(result.returncode)+"\n")
(out/(name+"-after.json")).write_text(json.dumps(freeze(),indent=2))
print(name, "exit", result.returncode)
sys.exit(result.returncode)
