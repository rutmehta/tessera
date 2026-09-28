from pathlib import Path
import json,hashlib,subprocess,struct
ROOT=Path(__file__).resolve().parents[1];OUT=Path(__file__).resolve().parent
for name in ('A','Google255'):
 r=json.loads((OUT/f'{name}.swap.json').read_text());a=Path(r['source']).read_bytes();b=Path(r['copy']).read_bytes();s,e=r['swapped_byte_range']
 assert len(a)==len(b)==r['bytes'] and a[:s]==b[:s] and a[e:]==b[e:]
 assert a[r['aux_soi_offset']:r['aux_soi_offset']+2]==b'\xff\xd8'
 assert b[r['aux_soi_offset']:r['aux_soi_offset']+2]==b'\xff\xd8'
 m0,m1=r['source_markers'][:2];c0,c1=r['copy_markers'][:2]
 assert a[m0['start']:m0['end']]==b[c1['start']:c1['end']]
 assert a[m1['start']:m1['end']]==b[c0['start']:c0['end']]
 for k in ('ICC_Profile','MPImage2'):
  x=subprocess.run(['exiftool','-b',f'-{k}',r['source']],capture_output=True,check=True).stdout
  y=subprocess.run(['exiftool','-b',f'-{k}',r['copy']],capture_output=True,check=True).stdout
  if k=='ICC_Profile':assert x==y
  else:
   # Exact auxiliary payload order differs, but size and compressed tail after both segments are unchanged.
   assert len(x)==len(y) and x[e-r['aux_soi_offset']:]==y[e-r['aux_soi_offset']:]
 assert r['mpf_primary'][1]==r['aux_soi_offset'] and r['aux_soi_offset']+r['mpf_auxiliary'][1]==len(a)
 # ISO payload is one original segment reused unmodified at new position.
 iso=[m for m in r['source_markers'] if m['label']=='APP2-ISO'];assert len(iso)==1
 data=a[iso[0]['start']:iso[0]['end']]
 assert data in b[s:e]
 info={'name':name,'source_sha256':hashlib.sha256(a).hexdigest(),'copy_sha256':hashlib.sha256(b).hexdigest(),'file_length':len(a),'mpf_aux_offset_and_size':[r['aux_soi_offset'],r['mpf_auxiliary'][1]],'outside_segment_pair_equal':True,'segment_contents_reused_whole':True,'iso_segment_equal':True,'icc_equal':True,'compressed_suffix_equal':True}
 (OUT/f'{name}.static.json').write_text(json.dumps(info,indent=2)+'\n')
 print(name,info)
