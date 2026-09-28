# Editable Smart Previews

User-authorized implementation, 2026-09-28. A owns engine/storage/Develop/Library; B retains Document/Save As. The user wants faster editing of reduced RAW proxies, with originals untouched. Offline editing and original-quality export are part of the stated behavior.

## Contract

Generate a bounded camera-linear RGB representation before camera calibration, white balance, tone and recipe geometry. Long edge is at most 2560 pixels; reduce demosaiced linear pixels, never decimate CFA. Preserve original camera metadata and the resolved upstream reconstruction/demosaic/lens prefix. Lens Auto and CA defaults remain supported when their actual operations are admitted. Prefix changes require regeneration from the original; unsupported late sensor opcodes must explicitly require the original. WB/exposure/tone/geometry remain editable. Proxy detail is an approximation, not original 1:1 detail.

Keep catalog/recipe ImageId and RAW source semantics. Decoded proxy identity is separate and includes representation, generator and payload identity. No working-Rec2020 RGB shortcut, developed-DNG export reuse, clipped JPEG proxy or fabricated camera-neutral metadata. Initial rendering uses the CPU route until other routes explicitly support camera-linear data.

Persist the proxy and recipe journal on local app-support storage independent of the photo volume. Bind to the original content digest/length, generation settings, calibration and payload integrity. Store exact recipe data including unknown members. A shared stable per-image writer admission must cover normal and proxy editors/setters; retain destination-based publication protection. Offline save is durable before reporting success. Reconnect preserves both sides when original content or owned sidecars changed. Dirty local edits must not be discarded by a proxy-cleanup operation.

Full-quality export uses the matching original and latest reconciled recipe. Missing originals produce an explicit unavailable state, never silent proxy upscaling. Library provides build/discard progress and visible editing-source/offline/stale state. Existing original and display-preview behavior stays covered by regression tests.

## Format ruling

The earlier product specification proposes lossy DNG, but the repository only supplies developed/uncompressed DNG paths. Implement and qualify the in-memory pre-edit boundary first. The persistent codec must be separately versioned and bounded and preserve negative/highlight values; an internal compressed camera-linear format is acceptable if measurements show useful size and fidelity. Do not label it lossy DNG without a real compatible implementation. Codec selection is not permission to skip offline persistence, source identity or UI behavior.

## Acceptance

Discriminating synthetic RAW tests use asymmetric channels, nonidentity calibration and custom WB. Check scale-one reference agreement, downstream-edit independence, prefix rejection, default lens order, odd crop/orientation and finite HDR samples. Storage tests cover atomic reopen, corruption/version/owner mismatch, stale generation and dirty-discard refusal. Integration tests cover ordinary/proxy writer conflict, offline save/restart, reconnect conflict, source switching and original-only export. Real Sony ARW fixture is available on betterSSD; source hash remains unchanged. Final GUI acceptance is separate from component tests and remains blocked where protected macOS UI prevents access.

Source audits: `docs/coordination/SMART-PREVIEW-ENGINE-DESIGN.md` and `SMART-PREVIEW-STORAGE-DESIGN.md`. Those include bounded-component limitations; this shipping contract takes precedence over neutral-lens-only component scope.
