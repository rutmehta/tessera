"""Run each original and 640x128 source separately through local native probes."""
from pathlib import Path
import hashlib,json,os,plistlib,subprocess
out=Path(__file__).resolve().parents[1]
root=out.parent
probe=Path(__file__).resolve().parent
files={
 'uniform80':root/'bright-white/reference-16-bright255-explicit.jpg',
 'split80':root/'split-gain/reference-16-split-explicit.jpg',
 'A80':Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),
 'uniform640':out/'reference-16-uniform-explicit.jpg',
 'split640':out/'reference-16-split-explicit.jpg',
}
expected_dims={'uniform80':(80,16),'split80':(80,16),'A80':(80,16),'uniform640':(640,128),'split640':(640,128)}
sha=lambda b:hashlib.sha256(b).hexdigest()
def hfile(p):return sha(p.read_bytes())
manifest={'imageio_source_sha256':hfile(probe/'imageio-probe-geometry.m'),'imageio_binary_sha256':hfile(probe/'imageio-probe-geometry'),'coreimage_source_sha256':hfile(probe/'coreimage-probe-geometry.m'),'coreimage_binary_sha256':hfile(probe/'coreimage-probe-geometry'),'inputs':{},'runs':{'imageio':{},'coreimage':{}}}
for name,path in files.items():
    assert path.is_file(),path
    manifest['inputs'][name]={'path':str(path),'sha256':hfile(path),'bytes':path.stat().st_size,'dimensions_expected':expected_dims[name]}

# Separate process per source ensures any native warning prefix is attributed to that file.
for name,path in files.items():
    width,height=expected_dims[name]
    run_dir=out/'native'/name/'imageio';run_dir.mkdir(parents=True,exist_ok=True)
    cmd=[str(probe/'imageio-probe-geometry'),str(path)]
    env=os.environ.copy();env['PROBE_OUTPUT_DIR']=str(run_dir)
    p=subprocess.run(cmd,env=env,capture_output=True)
    (run_dir/'command.json').write_text(json.dumps({'argv':cmd,'cwd':str(Path.cwd()),'env':{'PROBE_OUTPUT_DIR':str(run_dir)}},indent=2)+'\n')
    (run_dir/'stdout.bin').write_bytes(p.stdout);(run_dir/'stderr.bin').write_bytes(p.stderr);(run_dir/'direct.exit').write_text(f'{p.returncode}\n')
    split=p.stdout.find(b'[\n');assert split>=0,(name,p.stdout[:300],p.stderr)
    prefix=p.stdout[:split];records=json.loads(p.stdout[split:]);assert len(records)==1
    (run_dir/'warning-prefix.txt').write_bytes(prefix)
    record=records[0];(run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    assert p.returncode==0 and record['source_created'] if 'source_created' in record else p.returncode==0
    for mode in ('sdr','hdr'):
        d=record[mode]
        assert d['image_created'] and d['pixels_decoded'] and d['width']==width and d['height']==height,(name,mode,d)
        assert d['nonfinite_rgb']==0 and d['alpha_min']>.99 and d['alpha_max']<1.01
        assert d['provider_bytes']==width*height*4 and len(d['provider_sha256'])==64
        icc=run_dir/f'{mode}-returned-colorspace-icc.icc'
        assert icc.is_file(),(name,mode,'missing returned colorspace ICC',icc)
        icc_bytes=icc.read_bytes()
        assert 0<len(icc_bytes)<65536,(name,mode,len(icc_bytes))
        assert len(icc_bytes)==d['input_icc_bytes'] and hfile(icc)==d['input_icc_sha256'],(name,mode,len(icc_bytes),d['input_icc_bytes'],hfile(icc),d['input_icc_sha256'])
    prop=Path(record['source_properties_plist']);assert prop.is_file(),(name,record['source_properties_plist'])
    prop_bytes=prop.read_bytes();props=plistlib.loads(prop_bytes);assert isinstance(props,dict) and len(props)>0
    record['source_properties_sha256']=hfile(prop);record['source_property_keys']=sorted(str(k) for k in props.keys())
    (run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    run_rec={'path':str(path),'sha256':hfile(path),'direct_exit':p.returncode,'warning_prefix':prefix.decode('utf-8','replace').strip().splitlines(),'warning_prefix_sha256':sha(prefix),'cgimage_width_height':[width,height],'provider_bytes_per_mode':{m:record[m]['provider_bytes'] for m in ('sdr','hdr')},'provider_sha256':{m:record[m]['provider_sha256'] for m in ('sdr','hdr')},'returned_icc':{m:{'path':str(run_dir/f'{m}-returned-colorspace-icc.icc'),'bytes':record[m]['input_icc_bytes'],'sha256':record[m]['input_icc_sha256']} for m in ('sdr','hdr')},'ordinary_source_properties':{'plist':str(prop),'bytes':len(prop_bytes),'sha256':hfile(prop),'keys':record['source_property_keys']},'sdr':record['sdr'],'hdr':record['hdr']}
    manifest['runs']['imageio'][name]=run_rec

for name,path in files.items():
    width,height=expected_dims[name]
    run_dir=out/'native'/name/'coreimage';run_dir.mkdir(parents=True,exist_ok=True)
    cmd=[str(probe/'coreimage-probe-geometry'),str(run_dir),str(path)]
    p=subprocess.run(cmd,capture_output=True)
    (run_dir/'command.json').write_text(json.dumps({'argv':cmd,'cwd':str(Path.cwd())},indent=2)+'\n')
    (run_dir/'stdout.bin').write_bytes(p.stdout);(run_dir/'stderr.bin').write_bytes(p.stderr);(run_dir/'direct.exit').write_text(f'{p.returncode}\n')
    records=json.loads(p.stdout);assert len(records)==1
    record=records[0];(run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    assert p.returncode==0 and record['base_created'] and record['gain_created'],(name,p.returncode,record,p.stderr)
    for mode in ('default_expand','explicit_gain'):
        r=record[mode];assert r['image_created'] and r['valid_pixels'] and r['width']==width and r['height']==height
        assert r['nonfinite']==0 and r['alpha_min']>.99 and r['alpha_max']<1.01
        raw=run_dir/f'{path.name}.{mode}.rgba-f32le';assert raw.is_file(),raw
        b=raw.read_bytes();assert len(b)==width*height*16,(name,mode,len(b),width*height*16)
        r['rgba_f32le']={'path':str(raw),'bytes':len(b),'sha256':hfile(raw)}
    (run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    manifest['runs']['coreimage'][name]={'path':str(path),'sha256':hfile(path),'direct_exit':p.returncode,'default_expand':record['default_expand'],'explicit_gain':record['explicit_gain']}

# Baseline guard: generalized sample coordinates match the previous fixed80 values and headroom.
old_imageio=json.loads((root/'split-gain/native/imageio-parsed.json').read_text())
old_core=json.loads((root/'split-gain/native/coreimage-parsed.json').read_text())
for name,old_index in [('uniform80',0),('split80',1),('A80',2)]:
    old=old_imageio[old_index]
    new=manifest['runs']['imageio'][name]
    for mode in ('sdr','hdr'):
        a=old[mode];b=new[mode]
        assert (a['width'],a['height'],a['headroom'])==(b['cgimage_width_height'][0],b['cgimage_width_height'][1],b['headroom'])
        assert abs(a['rgb_max']-b['rgb_max'])<1e-5 and abs(a['rgb_mean']-b['rgb_mean'])<1e-6
        old_p=[v[:3] for v in a['patch_centers']]
        new_p=[v['rgba'][:3] for v in b['patch_centers']]
        assert len(old_p)==len(new_p)==5 and all(max(abs(x-y) for x,y in zip(a,b))<1e-5 for a,b in zip(old_p,new_p)),(name,mode,old_p,new_p)
    assert manifest['runs']['imageio'][name]['warning_prefix']==['too few samples','too few samples'] if name in ('uniform80','split80') else True
    oldc=old_core[old_index];newc=manifest['runs']['coreimage'][name]
    for mode in ('default_expand','explicit_gain'):
        a=oldc[mode];b=newc[mode]
        assert abs(a['peak']-b['peak'])<1e-5 and a['content_headroom']==b['content_headroom']
        assert all(max(abs(x-y) for x,y in zip(p['rgba'][:3],oldp[:3]))<1e-5 for p,oldp in zip(b['patches'],a['patches'])),(name,mode)
manifest['baseline80_requalification']='pass: generalized normalized sample path matches previously captured80x16 ImageIO/CoreImage values within stated numeric tolerances'
(out/'native-geometry-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'80x16_baselines':'pass','ImageIO':{k:{'warning_prefix':v['warning_prefix'],'uniform_hdr_peak':v['hdr']['rgb_max'],'hdr_headroom':v['hdr']['headroom'],'hdr_samples':[p['rgba'][:3] for p in v['hdr']['patch_centers']]} for k,v in manifest['runs']['imageio'].items()},'CoreImage':{k:{'peak':v['default_expand']['peak'],'headroom':v['default_expand']['content_headroom'],'samples':[p['rgba'][:3] for p in v['default_expand']['patches']]} for k,v in manifest['runs']['coreimage'].items()}},indent=2))
