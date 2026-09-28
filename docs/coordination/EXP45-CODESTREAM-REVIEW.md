# EXP-45 current-control codestream review

Read-only Luna review of the actual pinned libultrahdr v1.4.0/cjpeg 3.2.0 phase7 controls under `/Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404`: `bright-white/base-bright255.jpg`, `split-gain/gain-split.jpg`, and `split-gain/final-aux.jpg`, compared with the retained A base/gain JPEGs. No builds, product edits or additional native probe were performed for this review.

Both A and the actual current primary control use baseline SOF0, 8-bit, one sequential scan and 4:4:4 sampling. Quantization and Huffman payloads/selectors match. Primary SOF/SOS component labels differ consistently: A 0/1/2, cjpeg 1/2/3. Both gain JPEGs are baseline grayscale80×16 with matching quantization and Huffman scan parameters; A labels the component0, cjpeg1. A includes unused second quantization/Huffman tables absent from cjpeg grayscale output. The reference producer’s final auxiliary preserves the cjpeg entropy scan and exact decoded samples, as recorded in phase7 final-aux-qualification.json.

The independent split-gain producer reproduces ImageIO’s approximately8 result while its reference decoder and Core Image reach16, so A’s component labels are not required for this failure. This does not establish a specific ImageIO algorithm or justify a product change.

Correction: an earlier agent comparison used a historical Google Skia128×128 fixture with4:2:0 sampling and different Huffman tables. That fixture is outside the current producer experiment. Those earlier differences must not be attributed to the actual phase7 control; this note supersedes that comparison.
