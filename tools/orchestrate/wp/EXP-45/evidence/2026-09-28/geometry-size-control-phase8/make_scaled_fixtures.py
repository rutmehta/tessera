from pathlib import Path
import hashlib, json
root=Path(__file__).resolve().parents[1]
out=Path(__file__).resolve().parent
scale=8

def read_pnm(path, magic, w, h):
    data=path.read_bytes()
    header=f'{magic}\n{w} {h}\n255\n'.encode()
    assert data.startswith(header), (path, data[:40])
    assert len(data)==len(header)+w*h*(3 if magic=='P6' else 1)
    return header, data[len(header):]

def scale_samples(src, w, h, channels):
    # Exact nearest-neighbor integer replication, no interpolation.
    return bytes(src[((y//scale)*w+(x//scale))*channels+c]
                 for y in range(h*scale) for x in range(w*scale) for c in range(channels))

base_path=root/'bright-white/base-bright255.ppm'
base_header,base=read_pnm(base_path,'P6',80,16)
base_scaled_header=f'P6\n{80*scale} {16*scale}\n255\n'.encode()
base_scaled=base_scaled_header+scale_samples(base,80,16,3)
base_out=out/'base-bright255-640x128.ppm';base_out.write_bytes(base_scaled)

inputs={
  'uniform': root/'fixtures/gain.pgm',
  'split': root/'split-gain/gain-split.pgm',
}
records={'scale':scale,'dimensions':{'source':[80,16],'output':[80*scale,16*scale]},'method':'exact nearest-neighbor replication of every source sample; no filtering','base':{'source':str(base_path),'source_sha256':hashlib.sha256(base_path.read_bytes()).hexdigest(),'output':str(base_out),'output_sha256':hashlib.sha256(base_scaled).hexdigest(),'header_sha256':hashlib.sha256(base_scaled_header).hexdigest(),'unique_rgb':sorted(set(base))}}
for name,path in inputs.items():
    header,gain=read_pnm(path,'P5',80,16)
    scaled_header=f'P5\n{80*scale} {16*scale}\n255\n'.encode()
    scaled=scaled_header+scale_samples(gain,80,16,1)
    target=out/f'gain-{name}-640x128.pgm';target.write_bytes(scaled)
    rows=[]
    for y in range(16*scale):
        row=scaled[len(scaled_header)+y*80*scale:len(scaled_header)+(y+1)*80*scale]
        rows.append({'y':y,'zero':row.count(0),'max':row.count(255)})
    records[name]={'source':str(path),'source_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'output':str(target),'output_sha256':hashlib.sha256(scaled).hexdigest(),'header_sha256':hashlib.sha256(scaled_header).hexdigest(),'sample_counts':{str(v):scaled[len(scaled_header):].count(v) for v in sorted(set(gain))},'all_rows_same_counts':all((r['zero'],r['max'])==(rows[0]['zero'],rows[0]['max']) for r in rows),'row_counts':{'zero':rows[0]['zero'],'max':rows[0]['max']},'transition_x':40*scale if name=='split' else None}
(out/'fixture-qualification.json').write_text(json.dumps(records,indent=2)+'\n')
print(json.dumps(records,indent=2))
