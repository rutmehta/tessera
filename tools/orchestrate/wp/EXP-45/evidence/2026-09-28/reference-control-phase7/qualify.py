from pathlib import Path
import subprocess,hashlib,json,struct
ROOT=Path(__file__).resolve().parents[1];OUT=Path(__file__).resolve().parent
UNIFORM=ROOT/'bright-white/reference-16-bright255-explicit.jpg';SPLIT=OUT/'reference-16-split-explicit.jpg'
def sha(b):return hashlib.sha256(b).hexdigest()
def info(p):
 b=p.read_bytes();mpf=b.find(b'MPF\0');t=mpf+4;ifd=t+struct.unpack_from('>I',b,t+4)[0]
 tags={}
 for i in range(struct.unpack_from('>H',b,ifd)[0]):
  k,typ,n,v=struct.unpack_from('>HHII',b,ifd+2+i*12);tags[k]=(typ,n,v)
 e=t+tags[0xb002][2];attr,size0,off0=struct.unpack_from('>III',b,e);attr1,size1,off1=struct.unpack_from('>III',b,e+16)
 aux=t+off1;assert size0==aux and aux+size1==len(b)
 iso=b.find(b'urn:iso:std:iso:ts:21496:-1\0',aux);assert iso>=0
 return b,{'sha256':sha(b),'bytes':len(b),'mpf_aux_start':aux,'mpf_aux_size':size1,'mpf_primary_attribute':f'{attr:08x}','iso_payload':b[iso:iso+len(b'urn:iso:std:iso:ts:21496:-1\0')+5+56].hex(),'icc_sha256':sha(subprocess.run(['exiftool','-b','-ICC_Profile',str(p)],capture_output=True,check=True).stdout),'mpf_aux_size_field':[e+20,e+24]}
u,ui=info(UNIFORM);s,si=info(SPLIT)
assert ui['mpf_aux_start']==si['mpf_aux_start'] and ui['mpf_primary_attribute']==si['mpf_primary_attribute']=='00030000'
assert ui['iso_payload']==si['iso_payload'] and ui['icc_sha256']==si['icc_sha256']
start,end=ui['mpf_aux_size_field'];assert [start,end]==si['mpf_aux_size_field']
assert u[:start]==s[:start] and u[end:ui['mpf_aux_start']]==s[end:si['mpf_aux_start']]
# MPF size bookkeeping differs, and gain JPEG codestream/aux size may differ; base source, ICC and metadata are fixed.
for name,p in [('uniform',UNIFORM),('split',SPLIT)]:
 cfg=OUT/f'{name}.hdr.cfg';raw=OUT/f'{name}.hdr.raw';cmd=['./build/ultrahdr_app','-m','1','-j',str(p),'-o','0','-O','4','-f',str(cfg),'-z',str(raw)]
 r=subprocess.run(cmd,cwd=ROOT,capture_output=True)
 (OUT/f'{name}.decode.log').write_bytes(r.stdout+r.stderr)
 (OUT/f'{name}.decode.exit').write_text(str(r.returncode)+'\n')
 assert r.returncode==0
 pixels=raw.read_bytes();assert len(pixels)==80*16*8
 values=[struct.unpack_from('<e',pixels,i*8)[0] for i in range(1280)]
 centers=[values[8*80+x] for x in (13,39,40,50,53,66,79)]
 rec={'command':cmd,'exit':r.returncode,'hdr_linear_half_sha256':sha(pixels),'peak':max(values),'centers_x13_39_40_50_53_66_79':centers,'metadata':cfg.read_text()}
 (OUT/f'{name}.decode.json').write_text(json.dumps(rec,indent=2)+'\n')
 print(name,rec['peak'],centers)
 assert max(values)>=15.9
ui['reference_decode']=json.loads((OUT/'uniform.decode.json').read_text());si['reference_decode']=json.loads((OUT/'split.decode.json').read_text())
assert ui['reference_decode']['metadata']==si['reference_decode']['metadata']
(OUT/'static-qualification.json').write_text(json.dumps({'uniform':ui,'split':si,'primary_except_mpf_size_identical':True,'iso_identical':True,'icc_identical':True,'same_headroom_metadata':True},indent=2)+'\n')
