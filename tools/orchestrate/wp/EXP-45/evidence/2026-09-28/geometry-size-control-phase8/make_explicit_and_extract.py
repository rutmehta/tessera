"""Apply the already-qualified ISO common-denominator -> explicit rational rewrite."""
from pathlib import Path
import hashlib,json,struct
out=Path(__file__).resolve().parent
urn=b'urn:iso:std:iso:ts:21496:-1\0'
sha=lambda b:hashlib.sha256(b).hexdigest()
def rewrite(kind):
    src=out/f'reference-16-{kind}-common.jpg'; dst=out/f'reference-16-{kind}-explicit.jpg'
    raw=src.read_bytes(); assert raw[:2]==b'\xff\xd8'
    mpf=raw.find(b'MPF\0'); assert mpf>0
    t=mpf+4; assert raw[t:t+2]==b'MM'
    ifd=t+struct.unpack_from('>I',raw,t+4)[0]
    tags={}
    for i in range(struct.unpack_from('>H',raw,ifd)[0]):
        tag,typ,n,val=struct.unpack_from('>HHII',raw,ifd+2+i*12); tags[tag]=(typ,n,val)
    assert tags[0xb001]==(4,1,2) and tags[0xb002][:2]==(7,32)
    entries=t+tags[0xb002][2]
    attr0,size0,off0=struct.unpack_from('>III',raw,entries)
    attr1,size1,off1=struct.unpack_from('>III',raw,entries+16)
    assert attr0==0x00030000 and attr1==0 and off0==0
    aux=t+off1; assert size0==aux and aux+size1==len(raw)
    marker=aux+2; assert raw[marker:marker+2]==b'\xff\xe2'
    seglen=struct.unpack_from('>H',raw,marker+2)[0]
    oldseg=raw[marker:marker+2+seglen]; payload=raw[marker+4:marker+2+seglen]
    assert payload.startswith(urn)
    q=len(urn); assert payload[q:q+4]==bytes(4) and payload[q+4]==0x48
    vals=struct.unpack_from('>8I',payload,q+5)
    assert vals==(1,0,4,0,4,1,0,0) and len(payload)==q+5+32
    denom,base,alt,low,high,gamma,offb,offa=vals
    explicit=(base,denom,alt,denom,low,denom,high,denom,gamma,denom,offb,denom,offa,denom)
    newpayload=urn+bytes(4)+b'\x40'+struct.pack('>14I',*explicit)
    newseg=b'\xff\xe2'+struct.pack('>H',len(newpayload)+2)+newpayload
    assert len(newseg)-len(oldseg)==24
    target=bytearray(raw[:marker]+newseg+raw[marker+len(oldseg):])
    sizefield=entries+16+4
    before=struct.unpack_from('>I',target,sizefield)[0]; assert before==size1
    struct.pack_into('>I',target,sizefield,size1+24)
    assert target[:sizefield]==raw[:sizefield]
    assert target[sizefield+4:marker]==raw[sizefield+4:marker]
    assert target[marker+len(newseg):]==raw[marker+len(oldseg):]
    dst.write_bytes(target)
    # Extract the MPF secondary JPEG at the standards-derived offset for sample qualification.
    extracted=out/f'extracted-gain-{kind}-640x128.jpg'
    extracted.write_bytes(target[aux:aux+size1+24])
    return {'source':src.name,'source_sha256':sha(raw),'source_bytes':len(raw),'explicit':dst.name,'explicit_sha256':sha(target),'explicit_bytes':len(target),'secondary_range':[aux,aux+size1+24],'secondary_jpeg':extracted.name,'secondary_sha256':sha(extracted.read_bytes()),'iso_change':'ISO 0x48 common denominator to 0x40 explicit rationals using prior validated field parser','rational_pairs':[list(explicit[i:i+2]) for i in range(0,14,2)],'mpf_secondary_size_before':size1,'mpf_secondary_size_after':size1+24,'primary_attribute':f'{attr0:08x}'}
records={k:rewrite(k) for k in ('uniform','split')}
(out/'explicit-parser.json').write_text(json.dumps(records,indent=2)+'\n')
print(json.dumps(records,indent=2))
