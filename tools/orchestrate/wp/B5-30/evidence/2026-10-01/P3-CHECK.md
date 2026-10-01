# B5-30 on-screen P3 check — 2026-10-01, Machine B

Build 1d41a4ce (release). Display: built-in Liquid Retina XDR (M4 Max, 3456×2234, P3 panel). Each fixture launched in its own background window (`--nonactivating --app-dir <own dir> --open-document <png> --filter-selftest=<dir>`); Tessera never frontmost; windows captured with a ScreenCaptureKit helper (captures are in Display P3); pixels sampled at several points per half (all within 1/255). Fixtures from make-fixtures.swift: identical bytes, red half 255,0,0 and grey half 128, one tagged Display P3 and one sRGB (Apple's profile, so it takes the tagged path).

| fixture | canvas red | canvas grey | 1:1 Gaussian pane red / grey | status bar |
|---|---|---|---|---|
| P3-tagged (v-win-p3-p3cap.png) | 255, 0, 0 | 128, 128, 128 | 255, 0, 0 / 128, 128, 128 (v-win-p3-gauss.png) | 8-bit · Display P3 |
| sRGB-tagged (v-win-srgb.png) | 234, 51, 35 | 128, 128, 128 | 234, 51, 35 / 128, 128, 128 (v-win-srgb-gauss.png) | 8-bit · sRGB IEC61966-2.1 |

Result: PASS. Identical bytes render as full-gamut P3 red vs sRGB red re-expressed in P3 (HANDOFF predicted ≈235,51,35); greys match exactly; the detail pane matches its canvas in both documents. Gaussian Blur was opened from the menu and cancelled; neither document was modified. Both instances quit via Tessera ▸ Quit; no process left.
