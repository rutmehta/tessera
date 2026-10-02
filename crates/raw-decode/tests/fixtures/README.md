# Synthetic camera-channel JPEG

`linear-gradient.jpg` is a 16×16, quality-100, 4:4:4 JPEG generated solely
from integer ramps:

- component 0: `32 + 8*x`
- component 1: `48 + 7*y`
- component 2: `64 + 4*(x+y)`

`generate-gradient.rs` uses the already-locked `jpeg-encoder` 0.6.1. The
`ColorType::Ycbcr` input bypasses RGB-to-YCbCr encoding, so the component values
are the intended camera codes. The DNG wrapper supplies their LinearRaw
interpretation. This is not a display JPEG or a photograph.

The generator can be built using `rustc` against `jpeg-encoder`'s cached source
(`--crate-name jpeg_encoder --crate-type rlib --edition 2021 --cfg 'feature="std"'`),
then `--extern jpeg_encoder=<that rlib>`. Its only argument is the output path.
No workspace manifest changes are needed for regeneration.

The test wrapper writes classic TIFF entirely in memory, in both byte orders,
with the full image in a SubIFD. It tests tile/strip layout, a 256-entry code-to-
linear table, black/white levels, crop, orientation metadata, camera calibration,
and Adobe APP14 / RGB component-ID invariance. The gradient tolerance is
2.1 JPEG code values divided by the normalized black-to-white interval (254).

`linear-gradient.dng` is the synthetic JPEG above wrapped by
`tests/support::lossy_dng(false, false)`. It is used by the opt-in catalog fixture
builder, with no dependency on a RAW encoder or any photographic input.

`linear-gradient-jxl.dng` is the same synthetic TIFF as `linear-gradient.dng`,
with its JPEG payload replaced by `cjxl linear-gradient.jpg output.jxl
--lossless_jpeg=0 -d 0`; Compression is 52546 and TileByteCounts is updated.
It exercises the app's imported JPEG XL LinearRaw route without private pixels.
