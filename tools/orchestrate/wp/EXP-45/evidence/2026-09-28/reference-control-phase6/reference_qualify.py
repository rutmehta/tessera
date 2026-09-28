from pathlib import Path
import subprocess,hashlib,json,struct,shutil
ROOT=Path(__file__).resolve().parents[1];OUT=Path(__file__).resolve().parent
A=Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg')
G=ROOT/'bright-white/reference-16-bright255-explicit.jpg'
CASES={'A':(A,OUT/'A-aux-order-swapped.jpg'),'Google255':(G,OUT/'Google255-aux-order-swapped.jpg')}
def sha(b):return hashlib.sha256(b).hexdigest()
for name,(source,copy) in CASES.items():
 for variant,path in [('original',source),('swapped',copy)]:
  for fmt,fmtarg,transarg,bpp in [('hdr-linear-half','0','4',8),('sdr-srgb-u8','3','3',4)]:
   stem=f'{name}-{variant}.{fmt}';cfg=OUT/f'{stem}.cfg';raw=OUT/f'{stem}.raw';log=OUT/f'{stem}.log'
   cmd=['./build/ultrahdr_app','-m','1','-j',str(path),'-o',fmtarg,'-O',transarg,'-f',str(cfg),'-z',str(raw)]
   run=subprocess.run(cmd,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
   log.write_bytes(run.stdout)
   assert run.returncode==0,(cmd,run.stdout.decode(errors='replace'))
   data=raw.read_bytes();assert len(data)==80*16*bpp
   center=struct.unpack_from('<4e',data,(8*80+40)*8) if bpp==8 else tuple(data[(8*80+40)*4:(8*80+40)*4+4])
   meta=cfg.read_text()
   record={'command':cmd,'cwd':str(ROOT),'exit':run.returncode,'source_sha256':sha(path.read_bytes()),'raw_bytes':len(data),'raw_sha256':sha(data),'bright_center_rgba':center,'metadata':meta}
   (OUT/f'{stem}.json').write_text(json.dumps(record,indent=2)+'\n')
   print(stem,sha(data),center)
 for fmt in ('hdr-linear-half','sdr-srgb-u8'):
  o=json.loads((OUT/f'{name}-original.{fmt}.json').read_text());s=json.loads((OUT/f'{name}-swapped.{fmt}.json').read_text())
  assert o['raw_sha256']==s['raw_sha256'],(name,fmt)
  assert o['metadata']==s['metadata'],(name,fmt,'metadata')
 # MPF extraction ensures actual auxiliary bytes are re-ordered but same size; ICC unchanged.
 icc=[]
 for path in (source,copy):
  p=subprocess.run(['exiftool','-b','-ICC_Profile',str(path)],capture_output=True,check=True)
  icc.append(sha(p.stdout))
 assert icc[0]==icc[1]
 (OUT/f'{name}.qualification.json').write_text(json.dumps({'reference_hdr_bytes_identical':True,'reference_sdr_bytes_identical':True,'icc_sha256_both':icc[0]},indent=2)+'\n')
