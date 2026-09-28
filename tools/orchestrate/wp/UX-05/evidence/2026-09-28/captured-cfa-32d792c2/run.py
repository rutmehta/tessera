import subprocess,pathlib,hashlib,json,os,sys,datetime
root=pathlib.Path("/Users/rutmehta/.codex/worktrees/export-integration/tessera")
out=pathlib.Path(__file__).parent
name=sys.argv[1]; command=sys.argv[2:]
if (out/(name+".exit")).exists(): raise SystemExit("refusing to overwrite prior run")
config=json.loads((out/"fixture-env.json").read_text())
def fixtures():
    values={}
    for name,item in config["files"].items():
        p=pathlib.Path(item["path"])
        if not p.is_file(): raise RuntimeError("missing required family "+name)
        value={"bytes":p.stat().st_size,"sha256":hashlib.sha256(p.read_bytes()).hexdigest()}
        if value["bytes"]!=item["bytes"] or value["sha256"]!=item["sha256"]: raise RuntimeError("fixture identity mismatch "+name)
        values[name]=value
    if len(values)!=5: raise RuntimeError("five named families required")
    return values
def freeze():
    names=set(subprocess.check_output(["git","ls-files"],cwd=root,text=True).splitlines())
    names.update(subprocess.check_output(["git","ls-files","--others","--exclude-standard"],cwd=root,text=True).splitlines())
    return {"head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),"sources":{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sorted(names) if (root/p).is_file()}}
env=os.environ.copy();env.update(CARGO_TARGET_DIR="/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated",MACOSX_DEPLOYMENT_TARGET="15.0",CARGO_BUILD_JOBS="2",TESSERA_CAPTURED_CFA_FIXTURES=config["directory"],RAW_DECODE_FIXTURES=config["directory"])
(out/(name+"-command.json")).write_text(json.dumps({"command":command,"env":{k:env[k] for k in ["CARGO_TARGET_DIR","MACOSX_DEPLOYMENT_TARGET","CARGO_BUILD_JOBS","TESSERA_CAPTURED_CFA_FIXTURES","RAW_DECODE_FIXTURES"]},"start":datetime.datetime.now(datetime.timezone.utc).isoformat()},indent=2))
try:
    (out/(name+"-fixtures-before.json")).write_text(json.dumps(fixtures(),indent=2))
    (out/(name+"-before.json")).write_text(json.dumps(freeze(),indent=2))
except Exception as error:
    (out/(name+".log")).write_text(str(error));(out/(name+".exit")).write_text("2\n");raise SystemExit(2)
with (out/(name+".log")).open("w") as log:
    result=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
(out/(name+".exit")).write_text(str(result.returncode)+"\n")
(out/(name+"-after.json")).write_text(json.dumps(freeze(),indent=2))
(out/(name+"-fixtures-after.json")).write_text(json.dumps(fixtures(),indent=2))
print(name,"exit",result.returncode)
sys.exit(result.returncode)
