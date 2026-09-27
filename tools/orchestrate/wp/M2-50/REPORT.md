# M2-50 report: Photo Merge and Enhance in the app

RESULT: PASS (gate: 259 XCTest, 0 failures; bindings unchanged by build-ffi.sh). Commit a1466c3.

## Delivered
- Photo menu between Library and Develop: Photo Merge ▸ HDR… (⌃H), Panorama… (⌃M), HDR Panorama…; Enhance… (⌃⌥I); Cancel <running job>. Merge needs 2+ engine photos (HDR Panorama 4+, HDR ≤64, others ≤128); Enhance 1+. Dimmed in Layered Documents, People view, stub libraries and while a job runs.
- Photo Merge sheet (Sources/Tessera/Photo/PhotoMergeSheet.swift): engine `merge_preview` in a preview well, re-requested off the main actor with a 250 ms debounce when an engine-read option changes (stale results dropped). HDR: Auto Align, Deghost None/Low/Medium/High. Panorama: Projection Auto/Spherical/Cylindrical/Perspective, focal length for curved projections, Boundary Warp 0–100, Fill Edges. HDR Panorama: frames per bracket in selection order. All: Auto Settings (auto_tone), Create Stack. Engine preview warnings + app-side exposure-spread/missing-exposure advice from EXIF; engine notes as hints; footer explains a dimmed Merge.
- Enhance sheet (Sources/Tessera/Photo/EnhanceSheet.swift): Denoise + Amount, Super Resolution, Raw Details disabled with reason, "Download missing models" (allow_model_download, off by default), note that no before/after preview exists, last run's error shown on reopen.
- Background jobs (Sources/TesseraCore/Photo/PhotoJobController.swift): PhotoJobListener relay to the main actor; blocking wait() only in a detached task; progress strip above the status bar with stage names/counts and Cancel; indeterminate during model download; late events ignored; engine errors rewritten into actionable text.
- Completion: toast/status ("HDR merge: created X-HDR.dng", "Stacked with 3 source photos"); library syncs through the change feed and selects the new DNG(s), adding them to the view if the filter would hide them; files published before a later failure/cancel are also pulled in.
- Tests (Tests/TesseraCoreTests/PhotoMergeEnhanceTests.swift, 12): stubbed engine — option→FFI mapping, sheet problems, enable rules (incl. AppModel), EXIF parsing/spread warnings, stage titles/error text, preview/progress/completion, cancel with a late event, start failure, missing-weights failure. Real engine — three bracketed float LinearRaw DNGs written in the test, merged to -HDR.dng; checks the 4-image stack, focus/selection via the change feed, opens in Develop at 192 px wide.
- Docs: ACCEPTANCE §AB steps 540–549 + identifiers appendix; DESIGN.md paragraph (no new colours/sizes/fonts). ThemeLint green.

## Shared-file edits (marked M2-50)
AppModel.swift (photoJobs property); AppCommands.swift (Photo CommandMenu; Library+Photo wrapped in a Group because the commands builder hit its 10-item limit); ContentView.swift (progress strip + .photoJobSheets).

## FFI gaps (handled in UI)
No enhance preview call; model-download progress is start/ready only; no enhance-model cache check (missing model known only on failure); merge preview is an approximation; no exposure-spread analysis in the FFI (app-side from EXIF); exposure_values not exposed in the sheet; no stack UI in the grid; Raw Details intentionally unsupported.

## Not verified
No manual GUI run; ACCEPTANCE §AB covers it.
