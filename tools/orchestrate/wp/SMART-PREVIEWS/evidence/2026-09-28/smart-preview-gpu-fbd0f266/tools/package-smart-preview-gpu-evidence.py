from pathlib import Path
import hashlib,json,shutil,tarfile,subprocess
root=Path('/Volumes/betterSSD/tessera-validation/smart-previews/gpu-integration')
portable=root/'portable';portable.mkdir(exist_ok=True)
# Copy stable metadata and source snapshots; rendered pixels stay separately retained.
for p in root.rglob('*'):
 if not p.is_file() or portable in p.parents or p.name.startswith('portable') or p.suffix=='.rgb32f':continue
 if p.suffix not in {'.json','.log','.exit','.py','.rs','.md','.txt','.patch'}:continue
 d=portable/p.relative_to(root);d.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,d)
reviews=['tessera-smart-preview-gpu-consolidated-review.md','tessera-smart-preview-gpu-consolidated-notes.md','tessera-smart-preview-gpu-parity-diagnosis.md','tessera-smart-preview-hdr-normalization-review.md','tessera-smart-preview-gpu-default-route-review.md','tessera-smart-preview-gpu-rollout-assessment.md','tessera-preview-qualification-review.md','tessera-smart-preview-root-gate-verification.json','tessera-smart-preview-root-swift-verification.json','tessera-smart-preview-root-immutable-source-verification.json','tessera-smart-preview-fbd0f266-git-sha256.json']
for name in reviews:
 src=Path('/tmp')/name;assert src.is_file(),src
 dest=portable/'reviews'/name;dest.parent.mkdir(exist_ok=True);shutil.copy2(src,dest)
for src in [Path('/tmp/tessera-preview-qualification/run.py'),Path('/tmp/tessera-preview-qualification/compare.py'),Path('/tmp/tessera-preview-qualification/engine-iosurface-qualification.patch'),Path('/tmp/tessera-smart-preview-gpu-consolidated.patch'),Path('/tmp/tessera-gpu-candidate-evidence.py'),Path('/tmp/tessera-gpu-final-native-gates.py'),Path('/tmp/package-smart-preview-gpu-evidence.py')]:
 dest=portable/'tools'/src.name;dest.parent.mkdir(exist_ok=True);shutil.copy2(src,dest)
pixels={str(p.relative_to(root)):{'sha256':hashlib.file_digest(p.open('rb'),'sha256').hexdigest(),'bytes':p.stat().st_size} for p in root.rglob('*.rgb32f') if portable not in p.parents}
(portable/'RAW-PIXEL-ARTIFACTS.json').write_text(json.dumps({'retained_root':str(root),'included_in_portable':False,'reason':'roughly8GB retained separately; rawpixel hashes and all derivedcomparisons included','files':pixels},indent=2))
manifest={str(p.relative_to(portable)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in portable.rglob('*') if p.is_file() and p.name!='MANIFEST.json'}
(portable/'MANIFEST.json').write_text(json.dumps(manifest,indent=2))
archive=root/'portable-fbd0f266.tar.gz'
with tarfile.open(archive,'w:gz') as t:t.add(portable,arcname='smart-preview-gpu-fbd0f266')
(root/'portable-fbd0f266.sha256').write_text(hashlib.file_digest(archive.open('rb'),'sha256').hexdigest()+'  '+archive.name+'\n')
print('portable',len(manifest),'files',archive.stat().st_size,'bytes; rawpixels',len(pixels),sum(x['bytes'] for x in pixels.values()))
