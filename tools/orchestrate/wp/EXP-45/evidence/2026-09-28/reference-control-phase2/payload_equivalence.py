"""Read-only JPEG entropy and ICC equivalence check for EXP-45 copies."""
import hashlib
import json
import struct
from pathlib import Path
root=Path(__file__).resolve().parents[1]

def sha(x):return hashlib.sha256(x).hexdigest()
def extract(data, start, end):
    assert data[start:start+2]==b'\xff\xd8'
    p=start+2;icc=[]
    while p<end:
        assert data[p]==0xff,(p,data[p])
        marker=data[p+1]
        if marker==0xd9:break
        assert marker not in (0xd8,0x00) and p+4<=end
        length=struct.unpack_from('>H',data,p+2)[0]
        payload=data[p+4:p+2+length]
        if marker==0xe2 and payload.startswith(b'ICC_PROFILE\0'):
            icc.append(payload)
        if marker==0xda:
            entropy_start=p+2+length
            eoi=data.rfind(b'\xff\xd9',entropy_start,end)
            assert eoi>=entropy_start
            return {'entropy_offset':entropy_start,'entropy_sha256':sha(data[entropy_start:eoi]),'entropy_bytes':eoi-entropy_start,'icc_segment_count':len(icc),'icc_payload_sha256':sha(b''.join(icc)) if icc else None,'icc_payload_bytes':sum(map(len,icc))}
        p+=2+length
    raise ValueError('SOS missing')

for cap in (4,16):
    a=(root/f'fixtures/reference-{cap}.jpg').read_bytes()
    b=(root/f'denominator-explicit/reference-{cap}-explicit.jpg').read_bytes()
    aux=1479
    old={'base':extract(a,0,aux),'gain':extract(a,aux,len(a))}
    new={'base':extract(b,0,aux),'gain':extract(b,aux,len(b))}
    for part in ('base','gain'):
        assert old[part]['entropy_sha256']==new[part]['entropy_sha256']
        assert old[part]['icc_payload_sha256']==new[part]['icc_payload_sha256']
    out={'cap':cap,'source_sha256':sha(a),'explicit_sha256':sha(b),'source':old,'explicit':new,'base_and_gain_entropy_identical':True,'icc_payload_identical':True}
    (root/f'denominator-explicit/payload-equivalence-{cap}.json').write_text(json.dumps(out,indent=2)+'\n')
    print(cap,'base_entropy',old['base']['entropy_sha256'],'gain_entropy',old['gain']['entropy_sha256'],'ICC',old['base']['icc_payload_sha256'])
