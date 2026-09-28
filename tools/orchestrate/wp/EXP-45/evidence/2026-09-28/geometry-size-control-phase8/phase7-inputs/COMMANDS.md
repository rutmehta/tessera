# Phase 7 command provenance

Commands below transcribe actual executed tool calls. `encode.json`, `uniform.decode.json`, `split.decode.json`, `native/*.command.json`, direct `.exit` files, and logs retain machine-readable commands or outcomes where captured. The inline final-auxiliary whole-file comparison first asserted equality and failed because libultrahdr added auxiliary JPEG metadata; the corrected check records identical entropy scan and exact decoded samples in `final-aux-qualification.json`. That initial Python assertion was visible in tool output but was not saved to a raw file, so no standalone exit/log is claimed.

```sh
python3 split-gain/make_split_pgm.py
/opt/homebrew/bin/cjpeg -quality 100 -grayscale -outfile split-gain/gain-split.jpg split-gain/gain-split.pgm
/opt/homebrew/bin/djpeg -grayscale -pnm -outfile split-gain/gain-split-decoded.pgm split-gain/gain-split.jpg
./build/ultrahdr_app -m 0 -i bright-white/base-bright255.jpg -g split-gain/gain-split.jpg -f fixtures/cap-16.cfg -z split-gain/reference-16-split-common.jpg
python3 split-gain/make_explicit.py
python3 split-gain/qualify.py
python3 split-gain/run_native.py
```

The cjpeg binary was previously recorded as `/opt/homebrew/bin/cjpeg`, SHA-256 `95f541951b21ed95e41099e517b3e30eb0b666d36bee7f270c063b37b5de9c99`, libjpeg-turbo 3.2.0. The pinned ultrahdr source revision is `d52a0d13814ca399fc8a07e23de1d2c63f0e8404`. `native/INPUTS.json` gives exact reused ImageIO/Core Image executable and input hashes. Upstream source/build binaries remain external and are excluded from the portable bundle.
