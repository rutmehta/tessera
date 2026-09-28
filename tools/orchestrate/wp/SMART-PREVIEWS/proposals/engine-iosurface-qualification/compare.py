#!/usr/bin/env python3
"""Pixel inspection is post-run, never part of listener latency. Standard library only."""
import argparse,array,json,math,pathlib,statistics,sys
p=argparse.ArgumentParser();p.add_argument('results');a=p.parse_args();root=pathlib.Path(a.results)
def read(folder,row):
 data=array.array('f');data.frombytes((folder/row['pixel_file']).read_bytes())
 if sys.byteorder!='little':data.byteswap()
 w,h=row['dimensions'];assert len(data)==w*h*3;return data,w,h
def viewport(folder,row):
 # Bilinear aspect-fit sampling approximation. Same requested content rectangle;
 # not a claim of physical-display presenter identity. No rotation in Sony fixture.
 data,w,h=read(folder,row);ow,oh=row['viewport'];result=[]
 for y in range(oh):
  fy=max(0,min(h-1,(y+.5)*h/oh-.5));y0=int(fy);y1=min(y0+1,h-1);ty=fy-y0
  for x in range(ow):
   fx=max(0,min(w-1,(x+.5)*w/ow-.5));x0=int(fx);x1=min(x0+1,w-1);tx=fx-x0
   for c in range(3):
    t=data[(y0*w+x0)*3+c]*(1-tx)+data[(y0*w+x1)*3+c]*tx
    b=data[(y1*w+x0)*3+c]*(1-tx)+data[(y1*w+x1)*3+c]*tx
    result.append(t*(1-ty)+b*ty)
 return result
def metrics(a,b):
 errors=sorted(abs(x-y) for x,y in zip(a,b,strict=True));return {'mae':statistics.fmean(errors),'rmse':math.sqrt(statistics.fmean(v*v for v in errors)),'p95_absolute_pixel_error':errors[int(.95*(len(errors)-1))],'max':errors[-1]}
sets={d.name:(d,json.loads((d/'results.json').read_text())) for d in root.iterdir() if (d/'results.json').exists()};report=[]
for name,(folder,data) in sets.items():
 if not name.startswith('proxy-gpu-'):continue
 cpu_name=name.replace('proxy-gpu-','proxy-cpu-');orig_name=name.replace('proxy-gpu-','original-gpu-')
 cpu,cdata=sets[cpu_name];orig,odata=sets[orig_name]
 def key(r):return tuple(r['viewport']),r['label']
 cmap={key(r):r for r in cdata['rows']};omap={key(r):r for r in odata['rows']}
 for r in data['rows']:
  cr=cmap[key(r)];orr=omap[key(r)];assert r['settings']==cr['settings']==orr['settings'];same_dimensions=r['dimensions']==cr['dimensions'];g=viewport(folder,r);c=viewport(cpu,cr);o=viewport(orig,orr)
  result={'run':name,'viewport':r['viewport'],'label':r['label'],'gpu_level':r['level'],'cpu_level':cr['level'],'original_level':orr['level'],'dimensions':{'gpu':r['dimensions'],'cpu':cr['dimensions'],'original':orr['dimensions']},'cpu_proxy_vs_gpu_proxy':metrics(c,g),'original_vs_gpu_proxy_spatial_difference':metrics(o,g),'delivery_ms':{'original':orr['delivery_ms'],'cpu_proxy':cr['delivery_ms'],'gpu_proxy':r['delivery_ms']}}
  # Adaptive levels may differ; do not label speed ratio as equivalent-quality.
  result['same_proxy_render_dimensions']=same_dimensions
  if same_dimensions:
   rawg,_,_=read(folder,r);rawc,_,_=read(cpu,cr)
   tolerance=lambda ref:4/255 if r['format']=='sdr' else .002+.002*abs(ref)
   assert all(abs(x-y)<=tolerance(x) for x,y in zip(rawc,rawg,strict=True)),result
   result['matched_proxy_delivery_speedup']=cr['delivery_ms']/r['delivery_ms']
  report.append(result)
(root/'comparison.json').write_text(json.dumps(report,indent=2));print(f'Wrote {len(report)} per-frame comparisons; no aggregate latency percentile or quality claim.')
