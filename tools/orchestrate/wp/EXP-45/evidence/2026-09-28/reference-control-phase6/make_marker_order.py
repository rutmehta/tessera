from pathlib import Path
import hashlib,json,struct
ROOT=Path(__file__).resolve().parents[1]
OUT=Path(__file__).resolve().parent
A=Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg')
G=ROOT/'bright-white/reference-16-bright255-explicit.jpg'
CASES=[('A',A,'bb08f44df33f9426999edc116adc9fecd61a4f70a9fc351318071c155d2c413b',('APP0-JFIF','APP2-ISO')),('Google255',G,'61824f8eb85beb703d17c486e4c146c755ab6b6726213d4502aa6bf3b867f351',('APP2-ISO','APP0-JFIF'))]
def sha(b):return hashlib.sha256(b).hexdigest()
def mpf_aux(b):
 p=b.find(b'MPF\0');assert p>0;t=p+4;assert b[t:t+2]==b'MM';ifd=t+struct.unpack_from('>I',b,t+4)[0]
 tags={}
 for i in range(struct.unpack_from('>H',b,ifd)[0]):
  tag,typ,n,v=struct.unpack_from('>HHII',b,ifd+2+12*i);tags[tag]=(typ,n,v)
 assert tags[0xb002][:2]==(7,32)
 e=t+tags[0xb002][2];primary=struct.unpack_from('>III',b,e);aux=struct.unpack_from('>III',b,e+16)
 start=t+aux[2];assert primary[2]==0 and primary[1]==start and start+aux[1]==len(b)
 return start,primary,aux
def markers(b,start):
 assert b[start:start+2]==b'\xff\xd8';p=start+2;out=[]
 while True:
  assert b[p]==255
  m=b[p+1];assert m!=255
  if m==0xda:break
  assert m not in (0xd8,0xd9,0x01) and not 0xd0<=m<=0xd7
  length=struct.unpack_from('>H',b,p+2)[0];assert length>=2
  end=p+2+length;body=b[p+4:end]
  label=('APP0-JFIF' if m==0xe0 and body.startswith(b'JFIF\0') else 'APP2-ISO' if m==0xe2 and body.startswith(b'urn:iso:std:iso:ts:21496:-1\0') else f'0x{m:02x}')
  out.append((label,p,end,sha(b[p:end])))
  p=end
 return out
for name,path,expected,order in CASES:
 b=path.read_bytes();assert sha(b)==expected,(name,sha(b));start,primary,aux=mpf_aux(b);before=markers(b,start)
 assert tuple(x[0] for x in before[:2])==order,(name,before)
 x,y=before[:2];assert x[2]==y[1]
 c=b[:x[1]]+b[y[1]:y[2]]+b[x[1]:x[2]]+b[y[2]:]
 assert len(c)==len(b) and c[:x[1]]==b[:x[1]] and c[y[2]:]==b[y[2]:]
 assert mpf_aux(c)==(start,primary,aux)
 after=markers(c,start)
 assert [z[0] for z in after[:2]]==list(order[::-1])
 assert sorted(z[3] for z in before)==sorted(z[3] for z in after)
 assert [z[3] for z in before[2:]]==[z[3] for z in after[2:]]
 out=OUT/f'{name}-aux-order-swapped.jpg';out.write_bytes(c)
 record={'source':str(path),'source_sha256':sha(b),'copy':str(out),'copy_sha256':sha(c),'bytes':len(b),'aux_soi_offset':start,'mpf_primary':primary,'mpf_auxiliary':aux,'swapped_byte_range':[x[1],y[2]],'source_markers':[{'label':z[0],'start':z[1],'end':z[2],'sha256':z[3]} for z in before],'copy_markers':[{'label':z[0],'start':z[1],'end':z[2],'sha256':z[3]} for z in after],'outside_swapped_range_byte_identical':True,'all_whole_marker_contents_identical':True}
 (OUT/f'{name}.swap.json').write_text(json.dumps(record,indent=2)+'\n')
 print(name,'source',sha(b),'copy',sha(c),'aux',start,'range',x[1],y[2], 'order',[z[0] for z in before[:3]],'->',[z[0] for z in after[:3]])
