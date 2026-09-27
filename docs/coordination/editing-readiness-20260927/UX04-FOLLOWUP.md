# UX04 follow-up: Loupe disclosure, proof, and pointer checks — 2026-09-27

Manual GUI observations on the exact isolated validation candidate below. This note records only the tiny generated JPEG path; it does not extend acceptance to RAW, user libraries, or all editor tools.

## Candidate and fixture

- App bundle: `/Volumes/betterSSD/tessera-validation/loupe-review-candidate/Tessera-UX02b-16cb6ce5.app`
- Bundle ID: `dev.tessera.validation.ux02b.16cb6ce5`; executable: `TesseraReviewResumeDevTest`.
- Executable SHA-256: `7a54173b4aba0e2dd54b407cad97ba42fdd13899e22764a4ec0c5907cc68ef76`.
- Fixture: `/tmp/tessera-ux04-followup-20260927/library/Fixture long name - display proof test.jpg`; SHA-256 `7ea9ae26e75949e577d8a441b555e2ee4364a014ec034e7598e2a841d282b86e`. It is a generated 320×240 checkerboard JPEG, not a user photo.
- Dedicated support folder: `/tmp/tessera-ux04-followup-20260927/app-support`.
- CUA displayed a 1440×900 window initially; the actual app window was resized to 960×600 for the narrow-window checks.

## Observed checks

In Edit Photo, the long filename was visible in the editor header. Display info opened and exposed the actual display device/mode and headroom: `DELL S3221QS · linear extended · RGBA16F · EDR headroom 1.0× (max 1.0×)`. Escape dismissed the disclosure and left the photo editor active.

Shortcuts disclosure showed the workspace's navigation, Develop, Masks, undo, and Escape descriptions, including `Esc tool / Back to Library` for the JPEG edit route. Escape closed the disclosure while staying in the editor. These two checks observed no underlying navigation leak.

Soft Proofing was enabled with the available profile `Všeobecný CMYK profil`. The passive on-image label read `Proof · Všeobecný CMYK profil`; status reported `Proofing Všeobecný CMYK profil · 79% of colours out of gamut`. The Display info disclosure showed those details. Enabling gamut warning visibly added a magenta overlay and the status said `Gamut warning is on`.

At 960×600, the Display info and Shortcuts controls and the proof label remained visible. With proof still enabled, I activated Crop (`R`) and dragged its bottom-right crop handle. The dimensions changed from 320×240 to 301×221; cancel restored the full frame. At the larger window, the same pointer path changed 320×240 to 300×222 and cancel restored it. This verifies that the tested crop-handle drag reached the crop tool while the passive proof overlay was present. It does not verify mask-brush or color-picker interaction.

## Evidence and limits

The observations came from live CUA AX state and screenshots displayed during the session. No raw AX transcript or persistent screenshot file was saved, so the quoted UI strings above are session observations rather than attached capture artifacts. No source, build, GPU benchmark, or broad performance claim is part of this check.

This check covers a single generated JPEG, two disclosure Escape paths, a real CMYK soft-proof/gamut-warning state, the 960×600 window, and crop-handle pass-through. It does not establish RAW behavior, all pointer tools, arbitrary ICC profiles, all display configurations, or performance. A separate RAW export/reopen smoke was performed later; its XMP sidecar retains source edit metadata, so that separate run does not prove pixel parity or that reopening an exported JPEG cannot reapply recipe values. See `/tmp/tessera-raw-readiness-evidence.md` for its bounded observations and explicit limitations.
