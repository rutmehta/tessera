"""Scratch-only ISO common-denominator -> explicit rational control copy."""
import hashlib,json,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
SRC=ROOT/'bright-white/reference-16-bright255-common.jpg'
EXPECTED='b3da8f235a4551adba8e7f67e73d174b23fb093c79b071e05e9ef0a31fd082e7'
OUT=ROOT/'bright-white/reference-16-bright255-explicit.jpg'
URN=b'urn:iso:std:iso:ts:21496:-1\0'
sha=lambda data:hashlib.sha256(data).hexdigest()
raw=SRC.read_bytes();assert sha(raw)==EXPECTED
assert raw[:2]==b'\xff\xd8'
mpf=raw.find(b'MPF\0');assert mpf>0
tiff=mpf+4;assert raw[tiff:tiff+2]==b'MM'
ifd=tiff+struct.unpack_from('>I',raw,tiff+4)[0]
tags={}
for i in range(struct.unpack_from('>H',raw,ifd)[0]):
 tag,kind,n,val=struct.unpack_from('>HHII',raw,ifd+2+i*12);tags[tag]=(kind,n,val)
assert tags[0xb001]==(4,1,2) and tags[0xb002][:2]==(7,32)
e=tiff+tags[0xb002][2]
attr0,size0,off0=struct.unpack_from('>III',raw,e)
attr1,size1,off1=struct.unpack_from('>III',raw,e+16)
assert attr0==0x00030000 and attr1==0 and off0==0
aux=tiff+off1
assert aux==size0 and aux+size1==len(raw)
assert raw[aux:aux+2]==b'\xff\xd8' and raw[-2:]==b'\xff\xd9'
marker=aux+2
assert raw[marker:marker+2]==b'\xff\xe2'
seglen=struct.unpack_from('>H',raw,marker+2)[0]
oldseg=raw[marker:marker+2+seglen]
payload=raw[marker+4:marker+2+seglen]
assert payload.startswith(URN)
q=len(URN);assert payload[q:q+4]==bytes(4) and payload[q+4]==0x48
vals=struct.unpack_from('>8I',payload,q+5)
assert vals==(1,0,4,0,4,1,0,0)
assert len(payload)==q+5+32
denom,base,alt,low,high,gamma,offb,offa=vals
explicit=(base,denom,alt,denom,low,denom,high,denom,gamma,denom,offb,denom,offa,denom)
newpayload=URN+bytes(4)+b'\x40'+struct.pack('>14I',*explicit)
newseg=b'\xff\xe2'+struct.pack('>H',len(newpayload)+2)+newpayload
assert len(newseg)-len(oldseg)==24
output=bytearray(raw[:marker]+newseg+raw[marker+len(oldseg):])
sizefield=e+16+4
before=bytes(output[sizefield:sizefield+4]);assert struct.unpack('>I',before)[0]==size1
struct.pack_into('>I',output,sizefield,size1+24)
assert output[:sizefield]==raw[:sizefield]
assert output[sizefield+4:marker]==raw[sizefield+4:marker]
assert output[marker+len(newseg):]==raw[marker+len(oldseg):]
assert len(output)==len(raw)+24
OUT.write_bytes(output)
rec={'source':str(SRC),'source_sha256':sha(raw),'source_bytes':len(raw),'output':str(OUT),'output_sha256':sha(output),'output_bytes':len(output),'iso_aux_source_range':[marker,marker+len(oldseg)],'iso_aux_output_range':[marker,marker+len(newseg)],'iso_segment_length_before':seglen,'iso_segment_length_after':len(newpayload)+2,'mpf_aux_size_field_range':[sizefield,sizefield+4],'mpf_size_before_hex':before.hex(),'mpf_size_after_hex':output[sizefield:sizefield+4].hex(),'aux_soi_offset_unchanged':aux,'primary_mpf_attribute_unchanged':f'{attr0:08x}','common_values':list(vals),'explicit_rationals':[list(explicit[i:i+2]) for i in range(0,14,2)],'compressed_base_gain_and_unrelated_markers_unchanged':True}
(ROOT/'bright-white/make-explicit.json').write_text(json.dumps(rec,indent=2)+'\n')
print(json.dumps(rec,indent=2))
