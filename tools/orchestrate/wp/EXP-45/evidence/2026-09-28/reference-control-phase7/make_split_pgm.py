from pathlib import Path
import hashlib,json
ROOT=Path(__file__).resolve().parents[1];OUT=Path(__file__).resolve().parent
p=ROOT/'fixtures/gain.pgm';data=p.read_bytes();assert hashlib.sha256(data).hexdigest()=='9d9293b6cf23f44eb580d7a9066cd2439457310efcd3e6af3f323ddaa88050ab'
header=b'P5\n80 16\n255\n';assert data.startswith(header) and len(data)==len(header)+80*16
assert set(data[len(header):])=={255}
copy=bytearray(data)
for y in range(16):
 for x in range(40):copy[len(header)+80*y+x]=0
out=OUT/'gain-split.pgm';out.write_bytes(copy)
assert all(copy[len(header)+80*y+x] == (0 if x<40 else 255) for y in range(16) for x in range(80))
rec={'source':str(p),'source_sha256':hashlib.sha256(data).hexdigest(),'copy':str(out),'copy_sha256':hashlib.sha256(copy).hexdigest(),'header_sha256':hashlib.sha256(header).hexdigest(),'header_bytes':len(header),'changed_samples':640,'unchanged_samples':640,'sample_counts':{'0':640,'255':640},'all_rows_step_x40':True}
(OUT/'pgm-change.json').write_text(json.dumps(rec,indent=2)+'\n')
print(json.dumps(rec,indent=2))
