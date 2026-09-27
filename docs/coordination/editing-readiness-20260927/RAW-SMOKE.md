# Tessera RAW editing-readiness smoke — 2026-09-27

This was a bounded, manual GUI check with the already-built isolated validation app. It was not a benchmark, and it used one repository fixture copied into a disposable folder, not a user photo catalog.

## Exact inputs and app

- Fixture: `fixtures/raw/sony-arw.ARW`, copied to `/tmp/tessera-raw-edit-smoke-20260927/library/sony-arw.ARW`.
- Fixture SHA-256: `bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8`.
- ExifTool identification: Sony NEX-6, 4928×3276 (about 16.2 MP), 16.6 MB; original capture date 2017-01-07 10:57:05.
- App: `/Volumes/betterSSD/tessera-validation/loupe-review-candidate/Tessera-UX02b-16cb6ce5.app`; executable `TesseraReviewResumeDevTest`; bundle ID `dev.tessera.validation.ux02b.16cb6ce5`; executable SHA-256 `7a54173b4aba0e2dd54b407cad97ba42fdd13899e22764a4ec0c5907cc68ef76`.
- Dedicated app support: `/tmp/tessera-raw-edit-smoke-20260927/app-support`; JPEG export folder: `/tmp/tessera-raw-edit-smoke-20260927/export`.

## Observed workflow

Opened the RAW in Library and entered Edit Photo. The rendered photo and inspector controls became available. A basic edit was applied: Exposure +0.10 EV and Temperature 5132 K (tint remained +3). The app quit and relaunched; after reopening the fixture folder, the same settings appeared in Edit Photo. The recipe JSON and XMP sidecar recorded these values (`tone.exposure: 0.1`, `white_balance.temperature: 5132.284`, `tint: 3.4482`).

Exported with the built-in **Full-size JPEG** preset: JPEG quality 92, sRGB, original-size setting, 300 dpi, no watermark. The app reported “Exported 1 photo to export in 0.7 s.” Output: `/tmp/tessera-raw-edit-smoke-20260927/export/sony-arw.jpg`, 3.0 MB, SHA-256 `b2998d50ad73a2b60adba5bb9c1c16bf7ba39619c3262c8aa96207684b3c0a7a`. `sips` identified JPEG 4920×3276. This is the exporter’s observed output dimension, slightly narrower than the RAW’s 4928-pixel sensor dimension; no defect inference is made from that difference. Reopened the export folder in Tessera and saw the exported JPEG render in Library Loupe.

## Resource observations and limits

These are instantaneous snapshots only, not a peak measurement or a performance claim. During the first RAW editing process (PID 88146), a root-side sample observed about 718 MB RSS at roughly 28 seconds; later it was about 510 MB idle at 1:47. During the reopened-export app process (PID 93299), root captured 1,611,600 KiB RSS / 12.7% CPU around 33 seconds and 1,186,608 KiB / 13.8% CPU around 43 seconds; another sample later showed 378,768 KiB / 0% CPU at 1:16. These process snapshots came from different moments/stages and should not be read as a measured peak or a sustained-load result. The app was quit after the check; PID 93299 was absent afterward.

This verifies one Sony ARW open, basic adjustment persistence across quit/relaunch, one full-size JPEG export, and reopening that output. It does not establish RAW performance across cameras, large catalogs, batch processing, PSD, masking/retouch, AI, or broad editing readiness. No builds were run for this check; no user library or original photo was used.

## Subsequent source audit

The exported JPEG sidecar retains source develop adjustments. EXPORT-REOPEN-BUG.md
traces their reapplication when the baked JPEG is opened for editing. Rendering
and persistence observations above stand, but they do not prove export-reedit
fidelity. That readiness gate is blocked pending the narrow metadata fix.
