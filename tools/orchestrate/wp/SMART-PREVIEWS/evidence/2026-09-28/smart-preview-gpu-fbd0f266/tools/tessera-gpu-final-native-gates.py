import subprocess,os
run='/Volumes/betterSSD/tessera-validation/smart-previews/gpu-integration/run.py'
commands=[
 ('30-default-image-core',['cargo','test','-p','image-core','--test','smart_preview','--test','render','--release','--','--test-threads=1']),
 ('31-default-ffi-all',['cargo','test','-p','tessera-ffi','--lib','--release','--','--test-threads=1']),
 ('32-default-public-workflow',['cargo','test','-p','tessera-ffi','--test','smart_preview_workflow','--release','--','--ignored','--nocapture','--test-threads=1']),
 ('33-default-strict',['cargo','clippy','-p','pipeline-cpu','-p','image-core','-p','pipeline-gpu','-p','tessera-ffi','--all-targets','--release','--','-D','warnings']),
 ('34-default-fmt',['cargo','fmt','--all','--','--check'])]
env=dict(os.environ);env.pop('TESSERA_SMART_PREVIEW_GPU',None);env.pop('TESSERA_RENDER_BACKEND',None);env['TESSERA_SMART_PREVIEW_RAW']='/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW'
for tag,cmd in commands:
 result=subprocess.run(['python3',run,tag,*cmd],env=env)
 if result.returncode:raise SystemExit(result.returncode)
