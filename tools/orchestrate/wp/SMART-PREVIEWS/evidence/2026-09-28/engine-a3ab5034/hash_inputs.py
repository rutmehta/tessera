from pathlib import Path
import hashlib,sys
roots=['crates/pipeline-cpu','crates/raw-decode','crates/libraw-ffi','crates/engine-api','crates/lens','crates/color-mgmt','crates/typography','crates/vector','Cargo.toml','Cargo.lock','.cargo','rust-toolchain.toml']
files=set()
for name in roots:
 p=Path(name)
 if p.is_file():files.add(p)
 elif p.exists():files.update(x for x in p.rglob('*') if x.is_file() and '.git' not in x.parts)
Path(sys.argv[1]).write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p}\n' for p in sorted(files)))
