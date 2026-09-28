"""Run each original and 640x128 input separately through the native probes."""
from pathlib import Path
import hashlib,json,os,plistlib,subprocess,sys
out=Path(__file__).resolve().parents[1]
root=out.parent
probe=Path(__file__).resolve().parent
phase=sys.argv[1] if len(sys.argv)>1 else 'baseline'
assert phase in ('baseline','scaled')
files={
 'uniform80':root/'bright-white/reference-16-bright255-explicit.jpg',
 'split80':root/'split-gain/reference-16-split-explicit.jpg',
 'A80':Path('/Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera/tools/orchestrate/wp/M2-45d/a-gainmap-gates-20260927/headroom/artifacts/headroom-4-stops.jpg'),
 'uniform640':out/'reference-16-uniform-explicit.jpg',
 'split640':out/'reference-16-split-explicit.jpg',
}
selected=list(files.items())[:3] if phase=='baseline' else list(files.items())[3:]
expected_dims={'uniform80':(80,16),'split80':(80,16),'A80':(80,16),'uniform640':(640,128),'split640':(640,128)}
sha=lambda b:hashlib.sha256(b).hexdigest()
def hfile(p):return sha(p.read_bytes())
source_hashes={'imageio_source_sha256':hfile(probe/'imageio-probe-geometry.m'),'imageio_binary_sha256':hfile(probe/'imageio-probe-geometry'),'coreimage_source_sha256':hfile(probe/'coreimage-probe-geometry.m'),'coreimage_binary_sha256':hfile(probe/'coreimage-probe-geometry')}
manifest_path=out/'native-geometry-manifest.json'
if phase=='baseline':
    manifest={**source_hashes,'inputs':{},'runs':{'imageio':{},'coreimage':{}}}
else:
    assert manifest_path.is_file(),'baseline phase must finish and pass before scaled phase'
    manifest=json.loads(manifest_path.read_text())
    for key,value in source_hashes.items(): assert manifest[key]==value,(key,manifest[key],value)
for name,path in selected:
    assert path.is_file(),path
    manifest['inputs'][name]={'path':str(path),'sha256':hfile(path),'bytes':path.stat().st_size,'dimensions_expected':expected_dims[name]}

for name,path in selected:
    width,height=expected_dims[name]
    run_dir=out/'native'/name/'imageio';run_dir.mkdir(parents=True,exist_ok=True)
    cmd=[str(probe/'imageio-probe-geometry'),str(path)]
    env=os.environ.copy()
    # Keep the compiled probe's default decode/cache/float path deterministic.
    for key in ('PROBE_OPTIONS','PROBE_LUMA_OFF','PROBE_TARGET_ZERO'): env.pop(key,None)
    env['PROBE_OUTPUT_DIR']=str(run_dir)
    env['PROBE_DUMP']=str(run_dir)
    env['PROBE_RAW']=str(run_dir/'provider')
    env_record={k:env[k] for k in ('PROBE_OUTPUT_DIR','PROBE_DUMP','PROBE_RAW')}
    p=subprocess.run(cmd,env=env,capture_output=True)
    (run_dir/'command.json').write_text(json.dumps({'argv':cmd,'cwd':str(Path.cwd()),'controlled_env':env_record,'removed_option_overrides':['PROBE_OPTIONS','PROBE_LUMA_OFF','PROBE_TARGET_ZERO']},indent=2)+'\n')
    (run_dir/'stdout.bin').write_bytes(p.stdout);(run_dir/'stderr.bin').write_bytes(p.stderr);(run_dir/'direct.exit').write_text(f'{p.returncode}\n')
    split=p.stdout.find(b'[\n');assert split>=0,(name,p.stdout[:300],p.stderr)
    prefix=p.stdout[:split];records=json.loads(p.stdout[split:]);assert len(records)==1
    (run_dir/'warning-prefix.txt').write_bytes(prefix)
    record=records[0];(run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    assert p.returncode==0 and record.get('image_count')==1,(name,p.returncode,record,p.stderr)
    for mode in ('sdr','hdr'):
        d=record[mode]
        assert d['image_created'] and d['pixels_decoded'] and d['width']==width and d['height']==height,(name,mode,d)
        assert d['nonfinite_rgb']==0 and d['alpha_min']>.99 and d['alpha_max']<1.01
        assert d['provider_bytes']==width*height*4 and len(d['provider_sha256'])==64
        provider=run_dir/f'provider-{mode}.bin';drawn=run_dir/f'{path.name}.{mode}.f32'
        assert provider.is_file() and drawn.is_file(),(name,mode,provider,drawn)
        provider_bytes=provider.read_bytes();drawn_bytes=drawn.read_bytes()
        assert len(provider_bytes)==d['provider_bytes'] and hfile(provider)==d['provider_sha256']
        assert len(drawn_bytes)==width*height*16
        d['provider_dump']={'path':str(provider),'bytes':len(provider_bytes),'sha256':hfile(provider)}
        d['drawn_rgba_f32']={'path':str(drawn),'bytes':len(drawn_bytes),'sha256':hfile(drawn)}
        icc=run_dir/f'{mode}-returned-colorspace-icc.icc'
        assert icc.is_file(),(name,mode,'missing returned colorspace ICC',icc)
        icc_bytes=icc.read_bytes()
        assert 0<len(icc_bytes)<65536,(name,mode,len(icc_bytes))
        assert len(icc_bytes)==d['input_icc_bytes'] and hfile(icc)==d['input_icc_sha256'],(name,mode,len(icc_bytes),d['input_icc_bytes'],hfile(icc),d['input_icc_sha256'])
        d['returned_colorspace_icc']={'path':str(icc),'bytes':len(icc_bytes),'sha256':hfile(icc)}
    prop=Path(record['source_properties_plist']);assert prop.is_file(),(name,record.get('source_properties_plist'))
    prop_bytes=prop.read_bytes();props=plistlib.loads(prop_bytes);assert isinstance(props,dict) and len(props)>0
    record['source_properties_sha256']=hfile(prop);record['source_properties_bytes']=len(prop_bytes);record['source_property_keys']=sorted(str(k) for k in props.keys())
    (run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    manifest['runs']['imageio'][name]={'path':str(path),'sha256':hfile(path),'direct_exit':p.returncode,'warning_prefix':prefix.decode('utf-8','replace').strip().splitlines(),'warning_prefix_sha256':sha(prefix),'cgimage_width_height':[width,height],'provider':{m:record[m]['provider_dump'] for m in ('sdr','hdr')},'drawn_rgba_f32':{m:record[m]['drawn_rgba_f32'] for m in ('sdr','hdr')},'returned_icc':{m:record[m]['returned_colorspace_icc'] for m in ('sdr','hdr')},'ordinary_source_properties':{'plist':str(prop),'bytes':len(prop_bytes),'sha256':hfile(prop),'keys':record['source_property_keys']},'sdr':record['sdr'],'hdr':record['hdr']}

for name,path in selected:
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
        stem='default' if mode=='default_expand' else 'explicit'
        raw=run_dir/f'{path.name}.{stem}.rgba-f32le';assert raw.is_file(),raw
        b=raw.read_bytes();assert len(b)==width*height*16,(name,mode,len(b),width*height*16)
        r['rgba_f32le']={'path':str(raw),'bytes':len(b),'sha256':hfile(raw)}
    (run_dir/'result.json').write_text(json.dumps(record,indent=2)+'\n')
    manifest['runs']['coreimage'][name]={'path':str(path),'sha256':hfile(path),'direct_exit':p.returncode,'default_expand':record['default_expand'],'explicit_gain':record['explicit_gain']}

if phase=='baseline':
    old_imageio=json.loads((root/'split-gain/native/imageio-parsed.json').read_text())
    old_core=json.loads((root/'split-gain/native/coreimage-parsed.json').read_text())
    for name,old_index in [('uniform80',0),('split80',1),('A80',2)]:
        old=old_imageio[old_index];new=manifest['runs']['imageio'][name]
        for mode in ('sdr','hdr'):
            a=old[mode];b=new[mode]
            assert (a['width'],a['height'],a['headroom'])==(b['width'],b['height'],b['headroom'])
            assert abs(a['rgb_max']-b['rgb_max'])<1e-5 and abs(a['rgb_mean']-b['rgb_mean'])<1e-6
            old_p=[v[:3] for v in a['patch_centers']]
            new_p=[v['rgba'][:3] for v in b['patch_centers']]
            assert len(old_p)==len(new_p)==5 and all(max(abs(x-y) for x,y in zip(a,b))<1e-5 for a,b in zip(old_p,new_p)),(name,mode,old_p,new_p)
        oldc=old_core[old_index];newc=manifest['runs']['coreimage'][name]
        for mode in ('default_expand','explicit_gain'):
            a=oldc[mode];b=newc[mode]
            assert abs(a['peak']-b['peak'])<1e-5 and a['content_headroom']==b['content_headroom']
            assert all(max(abs(x-y) for x,y in zip(p['rgba'][:3],oldp[:3]))<1e-5 for p,oldp in zip(b['patches'],a['patches'])),(name,mode)
    manifest['baseline80_requalification']='pass: generalized normalized sample path matches previously captured80x16 ImageIO/CoreImage values within stated numeric tolerances'
else:
    manifest['scaled640_runs']='completed after baseline80_requalification'
manifest['last_phase']=phase
manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'phase':phase,'baseline80':manifest.get('baseline80_requalification'),'ImageIO':{k:{'warnings':v['warning_prefix'],'headroom':v['hdr']['headroom'],'peak':v['hdr']['rgb_max'],'centers':[p['rgba'][:3] for p in v['hdr']['patch_centers']]} for k,v in manifest['runs']['imageio'].items()},'CoreImage':{k:{'headroom':v['default_expand']['content_headroom'],'peak':v['default_expand']['peak'],'centers':[p['rgba'][:3] for p in v['default_expand']['patches']]} for k,v in manifest['runs']['coreimage'].items()}},indent=2))
