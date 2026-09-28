"""EXP-45 reciprocal scratch-only ICC APP2 profile-data swap."""
import hashlib,json,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'icc-swap'
SOURCES={
 'A':(Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),'bb08f44df33f9426999edc116adc9fecd61a4f70a9fc351318071c155d2c413b'),
 'Google255':(ROOT/'bright-white/reference-16-bright255-explicit.jpg','61824f8eb85beb703d17c486e4c146c755ab6b6726213d4502aa6bf3b867f351'),
}
def sha(data):return hashlib.sha256(data).hexdigest()
def inspect(b):
 assert b[:2]==b'\xff\xd8';pos=2;icc=[]
 while pos<len(b):
  assert b[pos]==255
  kind=b[pos+1]
  if kind==0xda:break
  assert kind not in (0xd8,0x00)
  length=struct.unpack_from('>H',b,pos+2)[0]
  assert length>=2
  payload=b[pos+4:pos+2+length]
  if kind==0xe2 and payload.startswith(b'ICC_PROFILE\0'):
   assert len(payload)>=14
   icc.append({'marker_offset':pos,'length_field':length,'segment_total':length+2,'chunk_prefix':payload[:14],'data_start':pos+18,'data_end':pos+2+length,'data':payload[14:]})
  pos+=2+length
 assert len(icc)==1
 x=icc[0]
 assert x['data_end']-x['data_start']==len(x['data'])
 assert x['chunk_prefix']==b'ICC_PROFILE\0\x01\x01'
 assert x['length_field']==604 and x['segment_total']==606 and len(x['data'])==588
 return x
read={}
for name,(p,expected) in SOURCES.items():
 b=p.read_bytes();assert sha(b)==expected
 x=inspect(b);(OUT/f'{name}.icc').write_bytes(x['data']);read[name]=(p,b,x)
assert read['A'][2]['chunk_prefix']==read['Google255'][2]['chunk_prefix']
assert read['A'][2]['length_field']==read['Google255'][2]['length_field']
for name,donor in [('A','Google255'),('Google255','A')]:
 path,b,x=read[name];donor_data=read[donor][2]['data']
 out=bytearray(b);out[x['data_start']:x['data_end']]=donor_data;out=bytes(out)
 assert len(out)==len(b)
 assert out[:x['data_start']]==b[:x['data_start']]
 assert out[x['data_end']:]==b[x['data_end']:]
 y=inspect(out);assert y['data']==donor_data and y['chunk_prefix']==x['chunk_prefix']
 changed=[i for i in range(x['data_start'],x['data_end']) if b[i]!=out[i]]
 output=OUT/f'{name}-with-{donor}-ICC.jpg';output.write_bytes(out)
 d={'source':str(path),'source_sha256':sha(b),'source_bytes':len(b),'donor':str(read[donor][0]),'donor_sha256':sha(read[donor][1]),'output':str(output),'output_sha256':sha(out),'output_bytes':len(out),'icc_segment_range':[x['marker_offset'],x['marker_offset']+x['segment_total']],'icc_data_range':[x['data_start'],x['data_end']],'icc_marker_length_field':x['length_field'],'icc_chunk_prefix_hex':x['chunk_prefix'].hex(),'source_icc_sha256':sha(x['data']),'donor_icc_sha256':sha(donor_data),'output_icc_sha256':sha(y['data']),'actual_changed_byte_count':len(changed),'actual_changed_byte_range':[min(changed),max(changed)+1] if changed else None,'all_outside_icc_data_bytes_identical':True,'file_length_unchanged':True}
 (OUT/f'{name}-swap.json').write_text(json.dumps(d,indent=2)+'\n')
 print(name,'source',sha(b),'output',sha(out),'profile',sha(x['data']),'->',sha(y['data']),'changedbytes',len(changed))
