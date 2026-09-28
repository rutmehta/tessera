"""EXP-45 scratch-only fixed-width ISO rational-representation swap."""
import hashlib, json, struct
from fractions import Fraction
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'rational-swap'
URN=b'urn:iso:std:iso:ts:21496:-1\0'
CASES={
 'A-to-reduced':{
  'source':Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),
  'sha':'bb08f44df33f9426999edc116adc9fecd61a4f70a9fc351318071c155d2c413b','expected':(4000000,1000000),'replace':(4,1),'mpf_attr':0x20030000,
 },
 'Google-to-million':{
  'source':ROOT/'denominator-explicit/reference-16-explicit.jpg',
  'sha':'cadfffeac4728b3536b738c6257bbc7c843bde5dc6153c160141fac5effc06ed','expected':(4,1),'replace':(4000000,1000000),'mpf_attr':0x00030000,
 },
}
def sha(x):return hashlib.sha256(x).hexdigest()
for name,case in CASES.items():
    src=case['source'];raw=src.read_bytes();assert sha(raw)==case['sha']
    urns=[];at=0
    while True:
        at=raw.find(URN,at)
        if at<0:break
        urns.append(at);at+=1
    assert len(urns)==2,urns
    aux_iso=urns[-1]
    header=aux_iso+len(URN)
    assert raw[header:header+4]==bytes(4) and raw[header+4]==0x40
    first=header+5
    old=list(struct.unpack_from('>14I',raw,first))
    assert tuple(old[2:4])==case['expected'] and tuple(old[6:8])==case['expected']
    assert Fraction(*old[2:4])==Fraction(4,1) and Fraction(*old[6:8])==Fraction(4,1)
    mpf=raw.find(b'MPF\0');assert mpf>0
    tiff=mpf+4;assert raw[tiff:tiff+2]==b'MM'
    ifd=tiff+struct.unpack_from('>I',raw,tiff+4)[0]
    entries=None
    for i in range(struct.unpack_from('>H',raw,ifd)[0]):
        tag,kind,count,value=struct.unpack_from('>HHII',raw,ifd+2+i*12)
        if tag==0xB002:assert (kind,count)==(7,32);entries=tiff+value
    assert entries is not None
    attr,size0,off0=struct.unpack_from('>III',raw,entries)
    attr1,size1,off1=struct.unpack_from('>III',raw,entries+16)
    assert attr==case['mpf_attr'] and attr1==0 and off0==0
    aux_start=tiff+off1
    assert aux_start==size0 and aux_start+size1==len(raw)
    assert raw[aux_start:aux_start+2]==b'\xff\xd8' and raw[-2:]==b'\xff\xd9'
    # Exact fixed-width word replacements. No APP2 length, MPF, ICC or JPEG byte moves.
    new=bytearray(raw);newpair=case['replace'];ranges=[]
    for pair_index in (2,6):
        offset=first+pair_index*4
        before=bytes(new[offset:offset+8]);after=struct.pack('>II',*newpair)
        assert struct.unpack('>II',before)==case['expected']
        new[offset:offset+8]=after
        ranges.append({'offset':offset,'end':offset+8,'before_hex':before.hex(),'after_hex':after.hex()})
    new=bytes(new)
    assert len(new)==len(raw)
    actual_diff={i for i,(a,b) in enumerate(zip(raw,new)) if a!=b}
    allowed={i for span in ranges for i in range(span['offset'],span['end'])}
    assert actual_diff<=allowed and actual_diff
    updated=list(struct.unpack_from('>14I',new,first))
    assert tuple(updated[2:4])==newpair and tuple(updated[6:8])==newpair
    assert [Fraction(updated[i],updated[i+1]) for i in range(0,14,2)]==[Fraction(old[i],old[i+1]) for i in range(0,14,2)]
    assert new[:first+8]==raw[:first+8] and new[first+32:]==raw[first+32:]
    out=OUT/f'{name}.jpg';out.write_bytes(new)
    record={'source':str(src),'source_sha256':sha(raw),'output':str(out),'output_sha256':sha(new),'bytes':len(new),'iso_aux_namespace_offset':aux_iso,'iso_flags_hex':'40','original_rationals':[old[2:4],old[6:8]],'replacement_rationals':[updated[2:4],updated[6:8]],'all_seven_numeric_rationals_equal':True,'changed_word_ranges':ranges,'actual_changed_byte_offsets':sorted(actual_diff),'mpf_primary_attr_hex':f'{attr:08x}','mpf_aux_offset':aux_start,'mpf_aux_size':size1,'length_and_all_other_bytes_unchanged':True}
    (OUT/f'{name}.json').write_text(json.dumps(record,indent=2)+'\n')
    print(name,sha(new),ranges)
