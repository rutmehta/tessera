# EXP-45 exact-white producer command record

This is a post-run transcription of the **actual commands used** for phase 4. The raw PPM mutation was executed as an inline Python heredoc; its resulting original/output hashes and exact changed region are in `base-ppm-change.json`. No separate direct exit file was captured for that inline Python command, so none is claimed. Its output file was then used by the commands below; no rerun was needed for this note.

The PPM mutation copied the already hashed `fixtures/base.ppm`, required header `P6\n80 16\n255\n`, and changed exactly each channel of x=53..79, y=0..15 from byte 240 to byte 255; all 2 earlier patches and all other bytes remained identical. The output is `base-bright255.ppm`, SHA-256 `3ac4503ac03416072c3d8ac4990c6e70f354a2d16b542396c33b2c7f0e6e175e`. The gain-map input `fixtures/gain.jpg` and metadata `fixtures/cap-16.cfg` remained the exact phase-1 files.

Actual JPEG conversion command (direct exit `cjpeg.exit` = 0; stdout/stderr saved separately):

```sh
cjpeg -quality 100 -sample 1x1 -outfile bright-white/base-bright255.jpg bright-white/base-bright255.ppm
```

The input base JPEG created thereby has SHA-256 `86996ff7e0600e6a7cd4eddca26290c9b5fe83d0f20f3c8ddbe6d8b3f4f78e1f`. It uses identical quality and sampling switches as phase 1. The observed executable path is `/opt/homebrew/bin/cjpeg`; its version/hash observed after the run are `libjpeg-turbo version 3.2.0 (build 20260630)` and SHA-256 `95f541951b21ed95e41099e517b3e30eb0b666d36bee7f270c063b37b5de9c99`, matching phase 1.

Actual pinned-codec encode command array/direct exit is preserved in `encode.json` (exit 0):

```sh
./build/ultrahdr_app -m 0 -i bright-white/base-bright255.jpg -g fixtures/gain.jpg -f fixtures/cap-16.cfg -z bright-white/reference-16-bright255-common.jpg
```

Actual pinned-codec decoder commands/exits are preserved in `decode-common.json` and `decode-explicit.json` (each exit 0). The scratch explicit-denominator copy script is `make_explicit.py` with direct `make-explicit.exit` 0 and exact changed ranges in `make-explicit.json`. It changed only the already qualified ISO representation and MPF auxiliary size, leaving the compressed base/gain and ICC bytes unchanged. The same phase-1 ImageIO and Core Image binaries were used without rebuilding; their hashes remain `950c42f9…` and `156d5595…` respectively, with full hashes in `MANIFEST.json`.
