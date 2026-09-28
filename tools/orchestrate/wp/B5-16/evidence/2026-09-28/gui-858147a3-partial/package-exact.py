import pathlib,subprocess,hashlib,json,shutil,plistlib
R=pathlib.Path('/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera')
E=pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/gui-858147a3')
S=pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-cd0b850d/swift-build/release')
F=pathlib.Path('/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW')
C='858147a3183c98b9e15118495a75da3bc2ddd0d0'
def sha(p):
 with open(p,'rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def tree(p):return {str(q.relative_to(p)):sha(q) for q in sorted(p.rglob('*')) if q.is_file() and not q.is_symlink()}
expected='f0f676ce47483531916923684b7c2ab817596d29b9c15c91c7e51363ec9d1171'
assert sha(pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/final-binaries/Tessera'))==expected
assert not E.exists(), 'Refuse to overwrite any prior package/profile'
inputs={'executable':sha(pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/final-binaries/Tessera')),'fixture':sha(F),'framework':tree(S/'Sparkle.framework')}
E.mkdir(parents=True)
shutil.copyfile(__file__,E/'package-exact.py')
commands=[]
def run(argv):
 p=subprocess.run(argv,capture_output=True,text=True)
 commands.append({'argv':argv,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
 (E/'commands.json').write_text(json.dumps(commands,indent=2))
 if p.returncode:raise RuntimeError(argv)
 return p.stdout
A=E/'Tessera Inspector 858147a3.app'
for d in ['Contents/MacOS','Contents/Resources','Contents/Frameworks']:(A/d).mkdir(parents=True,exist_ok=True)
for d in ['profile','photos','exports']:(E/d).mkdir()
shutil.copy2(pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/final-binaries/Tessera'),A/'Contents/MacOS/Tessera')
# No photograph copied or accessed for GUI documents
for rel,name in [('apps/mac/Support/Info.plist','source-Info.plist'),('apps/mac/Support/release/Tessera-adhoc.entitlements','Tessera-adhoc.entitlements')]:
 (E/name).write_bytes(subprocess.check_output(['git','show',C+':'+rel],cwd=R))
p=plistlib.loads((E/'source-Info.plist').read_bytes())
p.update(CFBundleIdentifier='dev.tessera.inspector-gui.858147a3',CFBundleName='Tessera Inspector 858147a3',CFBundleShortVersionString='0.0.1',CFBundleVersion='1')
(A/'Contents/Info.plist').write_bytes(plistlib.dumps(p))
(A/'Contents/PkgInfo').write_bytes(b'APPL????')
run(['ditto',str(S/'Sparkle.framework'),str(A/'Contents/Frameworks/Sparkle.framework')])
(E/'dependencies-before.txt').write_text(run(['otool','-L',str(A/'Contents/MacOS/Tessera')]))
(E/'load-commands-before.txt').write_text(run(['otool','-l',str(A/'Contents/MacOS/Tessera')]))
run(['install_name_tool','-add_rpath','@executable_path/../Frameworks',str(A/'Contents/MacOS/Tessera')])
after_rpath=sha(A/'Contents/MacOS/Tessera')
fw=A/'Contents/Frameworks/Sparkle.framework'
for rel in ['Versions/B/XPCServices/Downloader.xpc','Versions/B/XPCServices/Installer.xpc','Versions/B/Autoupdate','Versions/B/Updater.app']:
 run(['codesign','--force','--sign','-','--options','runtime','--preserve-metadata=entitlements',str(fw/rel)])
run(['codesign','--force','--sign','-','--options','runtime',str(fw)])
run(['codesign','--force','--sign','-','--options','runtime','--entitlements',str(E/'Tessera-adhoc.entitlements'),str(A)])
run(['codesign','--verify','--deep','--strict',str(A)])
run(['codesign','-d','--verbose=4',str(A)])
(E/'dependencies-after.txt').write_text(run(['otool','-L',str(A/'Contents/MacOS/Tessera')]))
(E/'load-commands-after.txt').write_text(run(['otool','-l',str(A/'Contents/MacOS/Tessera')]))
after={'executable':sha(pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/final-binaries/Tessera')),'fixture':sha(F),'framework':tree(S/'Sparkle.framework')}
assert inputs==after

manifest={'source_commit':C,'provenance':'strict05 Release executable; full04 source matched but relink distinguished','source_executable':str(pathlib.Path('/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/final-binaries/Tessera')),'source_fixture':str(F),'input_before':inputs,'input_after':after,'inputs_unchanged':inputs==after,'executable_after_rpath_sha256':after_rpath,'package_files_sha256':tree(A),'app':str(A),'profile':str(E/'profile'),'launched':False,'built':False,'changes':['unique Info.plist identifier/name/version','added executable-relative Frameworks rpath','ad-hoc signed copied Sparkle helpers/framework and bundle with immutable858147a3 entitlements']}
(E/'manifest.json').write_text(json.dumps(manifest,indent=2))
print(A)
