from pathlib import Path
import subprocess
import os
root=Path(__file__).resolve().parents[4]
audit=root/'tools/orchestrate/audits/layout'
build=audit/'swift-build/arm64-apple-macosx/debug'
objects=[]
for target in ['Tessera','TesseraCore','TesseraFFI']:
    objects += [str(p) for p in (build/(target+'.build')).glob('*.o') if p.name != 'TesseraApp.swift.o']
cmd=['swiftc','-parse-as-library','-I',str(build/'Modules'),'-I',str(root/'apps/mac/Sources/CTesseraFFI'),'-F',str(build),str(audit/'window-probe.swift'),*objects,str(root/'apps/mac/build/ffi/libtessera_ffi.a'),'-lc++','-lz','-framework','Security','-framework','CoreFoundation','-framework','Foundation','-framework','ImageCaptureCore','-framework','Sparkle','-Xlinker','-rpath','-Xlinker',str(build),'-o',str(audit/'window-probe')]
result=subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
(audit/'window-probe-build.log').write_text(result.stdout)
print(result.returncode,result.stdout[-5000:])
if result.returncode == 0:
    log_path = audit/('measure.log' if os.environ.get('TESSERA_AUDIT_MEASURE_ONLY') == '1' else 'window-probe.log')
    with log_path.open('w') as log:
        env = dict(os.environ, TESSERA_APP_DIR=str(audit/'app-state'))
        result=subprocess.run([str(audit/'window-probe'),str(audit/'shots'),str(root/'fixtures/raw')],env=env,stdout=log,stderr=subprocess.STDOUT,text=True,timeout=240)
    print(result.returncode,log_path.read_text()[-5000:])
