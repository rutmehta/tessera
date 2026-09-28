# Recovered tiny JPEG input provenance

This note is reconstructed from the **actual recorded tool-call commands** in the EXP-45 session. It was added after the diagnostic; no source, fixture, encoder, decoder, or native test was rerun. The original `MANIFEST.json` and original result/exit files remain unchanged. No separate exit code or stdout log was captured for the PPM/PGM creation or either `cjpeg` conversion, so none is claimed here. The resulting bytes and their hashes were captured immediately after creation and are recorded below.

The following Python command created `fixtures/base.ppm` (P6, 80×16, all RGB channels equal; x<26 has value 64, 26≤x<53 value 128, x≥53 value 240) and `fixtures/gain.pgm` (P5, 80×16, all samples 255), plus `cap-4.cfg` and `cap-16.cfg`. This is a verbatim reconstruction of the executed script:

```sh
python3 - <<'PY'
from pathlib import Path
p=Path('fixtures');p.mkdir(exist_ok=True)
w,h=80,16
rgb=bytearray()
for y in range(h):
 for x in range(w):
  v=64 if x<26 else 128 if x<53 else 240
  rgb.extend((v,v,v))
(p/'base.ppm').write_bytes(f'P6\n{w} {h}\n255\n'.encode()+rgb)
(p/'gain.pgm').write_bytes(f'P5\n{w} {h}\n255\n'.encode()+bytes([255])*(w*h))
for cap in (4,16):
 (p/f'cap-{cap}.cfg').write_text('\n'.join([f'--maxContentBoost {cap}', '--minContentBoost 1', '--gamma 1','--offsetSdr 0','--offsetHdr 0','--hdrCapacityMin 1', f'--hdrCapacityMax {cap}', '--useBaseColorSpace 1'])+'\n')
PY
```

The same shell invocation then executed, in this order:

```sh
cjpeg -quality 100 -sample 1x1 -outfile fixtures/base.jpg fixtures/base.ppm
cjpeg -quality 100 -grayscale -outfile fixtures/gain.jpg fixtures/gain.pgm
shasum -a 256 fixtures/*
```

The executable path observed from `command -v cjpeg` during setup and again during this provenance audit is `/opt/homebrew/bin/cjpeg`. Its version observed **after** encoding was `libjpeg-turbo version 3.2.0 (build 20260630)`; its SHA-256 observed after encoding was `95f541951b21ed95e41099e517b3e30eb0b666d36bee7f270c063b37b5de9c99`. Those observations establish current executable provenance, not a separately captured binary hash at the moment of each conversion.

| Output | SHA-256 captured after creation |
|---|---|
| `fixtures/base.ppm` | `63695e637ee121732e1c3cb21db95ef65962103bf922709fabeffd18e` |
| `fixtures/gain.pgm` | `9d9293b6cf23f44eb580d7a9066cd2439457310efcd3e6af3f323ddaa88050ab` |
| `fixtures/base.jpg` | `2832a2dde02912ee5f66af8ddfae7f21db95ef65962103bf922709fabeffd18e` |
| `fixtures/gain.jpg` | `3d93a0b1f14348bc40cbc55ebeed616fad771281a7e63ad3054f670e90a35907` |
| `fixtures/cap-4.cfg` | `5d869ae30a6993de071227b534ce21b7c3a774a9ae82c81bb836ae47c3662cb3` |
| `fixtures/cap-16.cfg` | `f598211428bcfce84fd98b931c99f165c8d61f5513d7f66b7b48f3139f85d973` |

The reference-codec encode command arrays and direct exits are separately captured in `encode-4.json` and `encode-16.json`; decoder commands/exits and linear output hashes are in `decode-*.json`. The external `MANIFEST.json` also retains hashes of the upstream source files, compiled library/application, and native probe binaries excluded from the portable bundle.
