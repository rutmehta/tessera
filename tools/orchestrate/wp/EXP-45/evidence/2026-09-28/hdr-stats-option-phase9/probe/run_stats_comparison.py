#!/usr/bin/env python3
"""Run ImageIO default vs HDR-stats on each 80x16 input in separate processes."""
from pathlib import Path
import hashlib, json, os, plistlib, subprocess, sys

out=Path(__file__).resolve().parents[1]
root=out.parent
probe=Path(__file__).resolve().parent
binary=probe/'imageio-probe-stats'
phase8= root/'size-640x128/native-geometry-manifest.json'
inputs={
 'uniform80':root/'bright-white/reference-16-bright255-explicit.jpg',
 'split80':root/'split-gain/reference-16-split-explicit.jpg',
 'A80':Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),
}
sha=lambda b:hashlib.sha256(b).hexdigest()
def hash_file(p):return sha(p.read_bytes())
assert binary.is_file() and phase8.is_file()
source_sha=hash_file(probe/'imageio-probe-stats.m')
binary_sha=hash_file(binary)
base_manifest=json.loads(phase8.read_text())
manifest={'phase':'phase9-hdr-stats-option','probe_source_sha256':source_sha,'probe_binary_sha256':binary_sha,
          'parent_probe_source_sha256':base_manifest['imageio_source_sha256'],
          'parent_probe_binary_sha256':base_manifest['imageio_binary_sha256'],
          'cases':{},'runs':{'default':{},'hdr_stats':{}}}
for name,path in inputs.items():
 assert path.is_file(),path
 input_hash=hash_file(path)
 assert base_manifest['inputs'][name]['sha256']==input_hash,(name,'input differs from retained phase8 fixture')
 manifest['cases'][name]={'source_path':str(path),'sha256':input_hash,'bytes':path.stat().st_size}
 for variant,enabled in [('default',False),('hdr_stats',True)]:
  run=out/'native'/variant/name/'imageio';run.mkdir(parents=True,exist_ok=False)
  cmd=[str(binary),str(path)]
  env=os.environ.copy()
  for key in ['PROBE_OPTIONS','PROBE_LUMA_OFF','PROBE_TARGET_ZERO','PROBE_HDR_STATS','PROBE_OUTPUT_DIR','PROBE_DUMP','PROBE_RAW']:
   env.pop(key,None)
  env['PROBE_OUTPUT_DIR']=str(run);env['PROBE_DUMP']=str(run);env['PROBE_RAW']=str(run/'provider')
  if enabled:env['PROBE_HDR_STATS']='1'
  controlled={k:env[k] for k in ['PROBE_OUTPUT_DIR','PROBE_DUMP','PROBE_RAW']}
  controlled['PROBE_HDR_STATS']='1' if enabled else 'unset'
  p=subprocess.run(cmd,env=env,capture_output=True,timeout=60)
  (run/'command.json').write_text(json.dumps({'argv':cmd,'cwd':str(Path.cwd()),'controlled_env':controlled,
      'cleared_inherited':['PROBE_OPTIONS','PROBE_LUMA_OFF','PROBE_TARGET_ZERO','PROBE_HDR_STATS']},indent=2)+'\n')
  (run/'stdout.bin').write_bytes(p.stdout);(run/'stderr.bin').write_bytes(p.stderr);(run/'direct.exit').write_text(f'{p.returncode}\n')
  split=p.stdout.find(b'[\n');assert split>=0,(name,variant,p.stdout[:300],p.stderr)
  warning=p.stdout[:split]; records=json.loads(p.stdout[split:]);assert len(records)==1
  (run/'warning-prefix.txt').write_bytes(warning)
  record=records[0]
  assert p.returncode==0 and record.get('image_count')==1,(name,variant,p.returncode,record,p.stderr)
  assert record['sdr']['decode_request_options']['compute_hdr_stats'] is False
  assert record['hdr']['decode_request_options']['compute_hdr_stats'] is enabled
  props_path=Path(record['source_properties_plist']);assert props_path.is_file()
  props=plistlib.loads(props_path.read_bytes());assert isinstance(props,dict)
  payloads={}
  for mode in ['sdr','hdr']:
   d=record[mode]
   assert d['image_created'] and d['pixels_decoded'] and (d['width'],d['height'])==(80,16)
   assert d['nonfinite_rgb']==0 and d['alpha_min']>.99 and d['alpha_max']<1.01
   provider=run/f'provider-{mode}.bin';draw=run/f'{path.name}.{mode}.f32';icc=run/f'{mode}-returned-colorspace-icc.icc'
   for file in [provider,draw,icc]: assert file.is_file(),file
   assert provider.stat().st_size==d['provider_bytes'] and hash_file(provider)==d['provider_sha256']
   assert draw.stat().st_size==80*16*16
   assert icc.stat().st_size==d['input_icc_bytes'] and hash_file(icc)==d['input_icc_sha256']
   payloads[mode]={'provider_bytes':provider.stat().st_size,'provider_sha256':hash_file(provider),
     'drawn_rgba_f32_bytes':draw.stat().st_size,'drawn_rgba_f32_sha256':hash_file(draw),
     'returned_icc_bytes':icc.stat().st_size,'returned_icc_sha256':hash_file(icc),
     'headroom':d['headroom'],'rgb_min':d['rgb_min'],'rgb_max':d['rgb_max'],'rgb_mean':d['rgb_mean'],
     'samples_above_one':d['samples_above_one'],'nonfinite_rgb':d['nonfinite_rgb'],
     'patch_centers':d['patch_centers'],'color_space':d['color_space']}
  record['source_properties_sha256']=hash_file(props_path)
  record['source_properties_bytes']=props_path.stat().st_size
  record['source_property_keys']=sorted(str(k) for k in props)
  record['payloads']=payloads
  (run/'result.json').write_text(json.dumps(record,indent=2)+'\n')
  if variant=='default':
   old=base_manifest['runs']['imageio'][name]['hdr'];cur=record['hdr']
   assert old['headroom']==cur['headroom'] and abs(old['rgb_max']-cur['rgb_max'])<1e-5,(name,'default HDR no-op baseline mismatch')
  manifest['runs'][variant][name]={'direct_exit':p.returncode,'warning_prefix':warning.decode('utf-8','replace').strip().splitlines(),
    'warning_prefix_sha256':sha(warning),'record':record}
# Compare default-process output to phase8, and enabled-process output to its paired default process.
for name in inputs:
 default=manifest['runs']['default'][name]['record']
 enabled=manifest['runs']['hdr_stats'][name]['record']
 old=base_manifest['runs']['imageio'][name]
 for mode in ['sdr','hdr']:
  prev=old[mode];cur=default[mode]
  assert (prev['headroom'],prev['rgb_max'])==(cur['headroom'],cur['rgb_max']) or (abs(prev['rgb_max']-cur['rgb_max'])<1e-5 and prev['headroom']==cur['headroom']),(name,mode,'default baseline mismatch')
 deltas={}
 for mode in ['sdr','hdr']:
  a=default['payloads'][mode];b=enabled['payloads'][mode]
  deltas[mode]={'provider_identical':a['provider_sha256']==b['provider_sha256'],
    'draw_identical':a['drawn_rgba_f32_sha256']==b['drawn_rgba_f32_sha256'],
    'returned_icc_identical':a['returned_icc_sha256']==b['returned_icc_sha256'],
    'same_headroom':a['headroom']==b['headroom'],'rgb_max_delta':b['rgb_max']-a['rgb_max'],
    'rgb_mean_delta':b['rgb_mean']-a['rgb_mean'],'same_samples_above_one':a['samples_above_one']==b['samples_above_one']}
 manifest['runs']['hdr_stats'][name]['delta_vs_default']=deltas
(out/'stats-comparison-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({n:{'default':manifest['runs']['default'][n]['record']['hdr']['headroom'],
 'enabled':manifest['runs']['hdr_stats'][n]['record']['hdr']['headroom'],
 'deltas':manifest['runs']['hdr_stats'][n]['delta_vs_default']} for n in inputs},indent=2))
