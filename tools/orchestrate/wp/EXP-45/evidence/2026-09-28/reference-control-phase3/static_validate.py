"""Independent read-only MPF, ISO, ICC, and byte-range checks for EXP-45 phase 3."""
import hashlib,json,struct,subprocess
from fractions import Fraction
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
U=b'urn:iso:std:iso:ts:21496:-1\0'
PAIRS=[('A-original',Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),ROOT/'rational-swap/A-to-reduced.jpg'),('Google-explicit',ROOT/'denominator-explicit/reference-16-explicit.jpg',ROOT/'rational-swap/Google-to-million.jpg')]
def sha(x):return hashlib.sha256(x).hexdigest()
def scan(path):
    b=path.read_bytes();m=b.find(b'MPF\0');assert m>=0
    t=m+4;assert b[t:t+2]==b'MM'
    ifd=t+struct.unpack_from('>I',b,t+4)[0]
    tags={}
    for i in range(struct.unpack_from('>H',b,ifd)[0]):
        tag,typ,n,val=struct.unpack_from('>HHII',b,ifd+2+i*12);tags[tag]=(typ,n,val)
    assert tags[0xb001]==(4,1,2) and tags[0xb002][:2]==(7,32)
    e=t+tags[0xb002][2]
    attr0,size0,off0=struct.unpack_from('>III',b,e)
    attr1,size1,off1=struct.unpack_from('>III',b,e+16)
    aux=t+off1
    assert off0==0 and size0==aux and aux+size1==len(b) and attr1==0
    extracted=subprocess.run(['exiftool','-b','-MPImage2',str(path)],capture_output=True,check=True).stdout
    assert extracted==b[aux:] and extracted[:2]==b'\xff\xd8' and extracted[-2:]==b'\xff\xd9'
    q=b.find(U,aux);assert q>=aux
    h=q+len(U);assert b[h:h+4]==bytes(4) and b[h+4]==0x40
    raw=struct.unpack_from('>14I',b,h+5)
    vals=[str(Fraction(raw[i],raw[i+1])) for i in range(0,14,2)]
    icc=subprocess.run(['exiftool','-b','-ICC_Profile',str(path)],capture_output=True,check=True).stdout
    return {'sha256':sha(b),'bytes':len(b),'mpf_primary_attr_hex':f'{attr0:08x}','mpf_aux_offset':aux,'mpf_aux_length':size1,'aux_sha256':sha(extracted),'iso_aux_namespace_offset':q,'iso_flags_hex':'40','rational_words':list(raw),'numeric_values':vals,'icc_exiftool_bytes':len(icc),'icc_exiftool_sha256':sha(icc) if icc else None}
for name,a,z in PAIRS:
    x,y=scan(a),scan(z)
    assert x['bytes']==y['bytes'] and x['mpf_primary_attr_hex']==y['mpf_primary_attr_hex'] and x['mpf_aux_offset']==y['mpf_aux_offset'] and x['mpf_aux_length']==y['mpf_aux_length']
    assert x['numeric_values']==y['numeric_values']==['0','4','0','4','1','0','0']
    assert x['icc_exiftool_sha256']==y['icc_exiftool_sha256']
    assert x['rational_words'][:2]==y['rational_words'][:2]
    assert x['rational_words'][4:6]==y['rational_words'][4:6]
    assert x['rational_words'][8:]==y['rational_words'][8:]
    rec={'source':str(a),'copy':str(z),'source_static':x,'copy_static':y,'all_numeric_rationals_equal':True,'mpf_and_icc_unchanged':True}
    (ROOT/'rational-swap'/f'{name}.static.json').write_text(json.dumps(rec,indent=2)+'\n')
    print(name,'original',x['rational_words'][2:4],x['rational_words'][6:8],'copy',y['rational_words'][2:4],y['rational_words'][6:8], 'aux',x['mpf_aux_offset'],x['mpf_aux_length'])
