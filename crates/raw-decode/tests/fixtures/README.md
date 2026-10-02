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

LR-8f explicitly adds Adobe APP14 transform 0 to the camera-code JPEG and its
synthetic DNG wrapper. JFIF without that marker denotes YCbCr and is converted
by the decoder; the pixel regression tests strip/replace APP14 to cover both.

`linear-gradient-16.jxl` is a 16x16 lossless RGB 16-bit synthetic ramp generated
by `generate-gradient-16.rs` with already-locked zune-jpegxl 0.5.2 and zune-core
0.5.3. Build the generator with rustc --edition 2024, passing their cached rlibs
with --extern and the release dependency directory with -L dependency. Its
output path is relative to the workspace root. No dependency change is needed.
The mutation wrapper adds a full 65536-entry identity LinearizationTable,
calibration, crop, ActiveArea, and all three opcode lists (identity MapPolynomial
in List2). Both full seeds must successfully decode before mutation begins.
