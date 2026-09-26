# Bundled test fonts

Unmodified upstream binaries, used only for offline deterministic tests.

## NotoSans-Regular.ttf

Source: https://github.com/notofonts/noto-fonts/blob/main/hinted/ttf/NotoSans/NotoSans-Regular.ttf

Copyright 2018 The Noto Project Authors (github.com/googlei18n/noto-fonts).
SIL Open Font License 1.1, included verbatim in `OFL.txt`.

SHA-256: `b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5`

The upstream font includes hint instructions; the renderer does not execute
those instructions. It uses unhinted ttf-parser outlines and tiny-skia AA.

## NotoSans-Variable.ttf

Source: https://github.com/google/fonts/blob/main/ofl/notosans/NotoSans%5Bwdth,wght%5D.ttf

Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic).
SIL Open Font License 1.1, included verbatim in `OFL-variable.txt`.

SHA-256: `bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d`

Exercises real `wght` and `wdth` variation in both shaping and outlines.

The binary checksums, not the moving upstream branches, pin the fixtures.
These fonts are not installed on the host and are not shipped as production
font dependencies by this library.
