from pathlib import Path
import json,hashlib
OUT=Path(__file__).resolve().parent;N=OUT/'native'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
a=json.loads((N/'imageio-parsed.json').read_text());c=json.loads((N/'coreimage-parsed.json').read_text())
records=[]
for x,y in zip(a,c):
 records.append({'file':Path(x['path']).name,'iso_aux_present':bool(x['iso_aux_present']),'sdr_peak':x['sdr']['rgb_max'],'hdr_peak':x['hdr']['rgb_max'],'imageio_headroom':x['hdr']['headroom'],'coreimage_peak':y['default_expand']['peak'],'coreimage_gain_present':bool(y['gain_created'])})
assert [r['imageio_headroom'] for r in records]==[8,8,16,16]
assert [r['iso_aux_present'] for r in records]==[True]*4
assert [r['hdr_peak'] for r in records[:2]]==[7.983762264251709]*2
assert [r['hdr_peak'] for r in records[2:]]==[15.951576232910156]*2
(N/'NATIVE-SUMMARY.json').write_text(json.dumps(records,indent=2)+'\n')
files=[{'path':str(p.relative_to(OUT)),'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(OUT.rglob('*')) if p.is_file() and p.name not in ('MANIFEST.json','RESULTS.md')]
(OUT/'MANIFEST.json').write_text(json.dumps({'phase':'aux-marker-order','files':files,'direct_exits':{p.name:p.read_text().strip() for p in OUT.rglob('*.exit')}},indent=2)+'\n')
print(len(files),json.dumps(records,indent=2))
