"""Read-only reciprocal ICC swap qualification for EXP-45 phase 5."""
import hashlib,json,struct,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
U=b'urn:iso:std:iso:ts:21496:-1\0'
CASES=[('A',Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),ROOT/'icc-swap/A-with-Google255-ICC.jpg',0x20030000),('Google255',ROOT/'bright-white/reference-16-bright255-explicit.jpg',ROOT/'icc-swap/Google255-with-A-ICC.jpg',0x00030000)]
def sha(x):return hashlib.sha256(x).hexdigest()
def parse(path):
 b=path.read_bytes();assert b[:2]==b'\xff\xd8'
 mpf=b.find(b'MPF\0');assert mpf>=0;t=mpf+4;assert b[t:t+2]==b'MM'
 ifd=t+struct.unpack_from('>I',b,t+4)[0]
 tags={}
 for i in range(struct.unpack_from('>H',b,ifd)[0]):
  tag,typ,n,v=struct.unpack_from('>HHII',b,ifd+2+12*i);tags[tag]=(typ,n,v)
 assert tags[0xb001]==(4,1,2) and tags[0xb002][:2]==(7,32)
 e=t+tags[0xb002][2];attr,size0,off0=struct.unpack_from('>III',b,e);attr1,size1,off1=struct.unpack_from('>III',b,e+16)
 aux=t+off1;assert off0==0 and attr1==0 and size0==aux and aux+size1==len(b)
 extracted=subprocess.run(['exiftool','-b','-MPImage2',str(path)],capture_output=True,check=True).stdout
 assert extracted==b[aux:]
 q=b.find(U,aux);assert q>=0;h=q+len(U);assert b[h:h+4]==bytes(4) and b[h+4]==0x40
 pairs=struct.unpack_from('>14I',b,h+5);assert [pairs[i]/pairs[i+1] for i in range(0,14,2)]==[0,4,0,4,1,0,0]
 icc=subprocess.run(['exiftool','-b','-ICC_Profile',str(path)],capture_output=True,check=True).stdout
 assert len(icc)==588
 return {'sha256':sha(b),'bytes':len(b),'mpf_primary_attr_hex':f'{attr:08x}','mpf_aux_offset':aux,'mpf_aux_size':size1,'aux_sha256':sha(extracted),'iso_secondary_offset':q,'iso_flags_hex':'40','iso_rational_words':pairs,'icc_bytes':len(icc),'icc_sha256':sha(icc)}
for name,src,copy,expected_attr in CASES:
 a,b=parse(src),parse(copy)
 assert int(a['mpf_primary_attr_hex'],16)==expected_attr
 for key in ('bytes','mpf_primary_attr_hex','mpf_aux_offset','mpf_aux_size','aux_sha256','iso_secondary_offset','iso_flags_hex','iso_rational_words'):
  assert a[key]==b[key],(name,key)
 out={'source':str(src),'copy':str(copy),'source':a,'copy_info':b,'all_mpf_iso_auxiliary_fields_unchanged':True,'icc_payload_swapped':a['icc_sha256']!=b['icc_sha256']}
 (ROOT/'icc-swap'/f'{name}.static.json').write_text(json.dumps(out,indent=2)+'\n')
 print(name,a['icc_sha256'],'->',b['icc_sha256'],'MPF',a['mpf_aux_offset'],a['mpf_aux_size'])
