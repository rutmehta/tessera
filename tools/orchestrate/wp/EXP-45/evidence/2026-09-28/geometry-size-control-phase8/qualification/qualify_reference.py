from pathlib import Path
import hashlib,json,struct
out=Path(__file__).resolve().parents[1];w,h=640,128
sha=lambda b:hashlib.sha256(b).hexdigest()
def pnm(path,magic):
    b=path.read_bytes(); parts=b.split(b'\n',3)
    assert parts[0]==magic.encode() and tuple(map(int,parts[1].split()))==(w,h) and parts[2]==b'255'
    assert len(parts[3])==w*h*(3 if magic=='P6' else 1)
    return parts[3]
result={'geometry':[w,h],'sample_coordinates':{'x':[64,192,320,448,576],'y':64,'normalized_fractions':[.1,.3,.5,.7,.9]},'gain':{},'reference':{},'base':{}}
for kind in ('uniform','split'):
    src=pnm(out/f'gain-{kind}-640x128.pgm','P5')
    decoded=pnm(out/f'extracted-gain-{kind}-640x128.pgm','P5')
    changed=sum(a!=b for a,b in zip(src,decoded)); assert changed==0,(kind,changed)
    counts={str(v):decoded.count(v) for v in sorted(set(decoded))}
    expected={'255':w*h} if kind=='uniform' else {'0':w*h//2,'255':w*h//2}
    assert counts==expected,(kind,counts,expected)
    rows_identical=all(decoded[y*w:(y+1)*w]==decoded[:w] for y in range(1,h));assert rows_identical
    step_ok=(kind=='uniform' or all(decoded[y*w:y*w+320]==bytes(320) and decoded[y*w+320:(y+1)*w]==bytes([255])*320 for y in range(h)))
    assert step_ok
    result['gain'][kind]={'source_sha256':sha(src),'decoded_sha256':sha(decoded),'exact_samples_match':True,'sample_counts':counts,'every_row_identical':rows_identical,'transition_x':320 if kind=='split' else None,'source_samples_per_row':640}
    raw=(out/f'{kind}.hdr.raw').read_bytes();assert len(raw)==w*h*8,(kind,len(raw))
    # Reference CLI output is RGBA16F; collect per-pixel R values and every-channel global maximum.
    rgba=[struct.unpack_from('<eeee',raw,i*8) for i in range(w*h)]
    r_values=[px[0] for px in rgba];global_peak=max(max(px[:3]) for px in rgba)
    centers=[rgba[64*w+x][0] for x in result['sample_coordinates']['x']]
    assert max(r_values)==global_peak and abs(global_peak-16.0)<0.1,(kind,global_peak)
    assert abs(centers[-1]-16)<.1 and abs(centers[-2]-16)<.1,(kind,centers)
    result['reference'][kind]={'raw_sha256':sha(raw),'raw_bytes':len(raw),'rgb_global_peak':global_peak,'alpha_min':min(px[3] for px in rgba),'alpha_max':max(px[3] for px in rgba),'centers_R':centers,'config_sha256':sha((out/f'{kind}.hdr.cfg').read_bytes()),'metadata':(out/f'{kind}.hdr.cfg').read_text()}
assert result['reference']['uniform']['metadata']==result['reference']['split']['metadata']
base=pnm(out/'decoded-base-640x128.ppm','P6');checks=[]
for y in (0,64,127):
    for x,expected in ((32,64),(240,128),(520,255)):
        rgb=tuple(base[(y*w+x)*3:(y*w+x)*3+3]);assert rgb==(expected,)*3,(x,y,rgb,expected)
        checks.append({'x':x,'y':y,'expected':expected,'decoded_rgb':rgb})
result['base']={'decoded_base_sha256':sha(base),'decoded_dimensions':[w,h],'base_level_counts_in_source':{'64':26*16*3,'128':27*16*3,'255':27*16*3},'interior_checks':checks,'source_replicated_exactly':True}
(out/'reference-and-sample-qualification.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
