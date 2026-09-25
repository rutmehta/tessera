# WP M2-13 — Develop panels UI: tone curve, HSL, colour grading, detail, effects, crop (Opus)

Read docs/01 §2.3–2.7, §2.11, §2.13, crates/pipeline-cpu OPERATORS.md (M2 operators), engine-api DevelopSettings (fields per stage), crates/tessera-ffi/src/develop.rs (session, set_settings JSON merge patch, interactive flag, commit), apps/mac Inspector (Basic panel, custom NSControl sliders, display-link coalescing).
Build the remaining Develop panels on the same session API with the same < 16 ms interactive path:
- Tone Curve: parametric (region sliders + split points) and point curve editor (luma + R/G/B, monotone cubic, draggable points, keyboard nudge), with the histogram behind it.
- HSL / Color: 8 hue bands × hue/sat/lum with a targeted-adjustment picker that samples the loupe and drags the corresponding band; B&W mix hidden until the field exists.
- Color Grading: three wheels (shadows/midtones/highlights) + global, blending/balance sliders, luminance per wheel.
- Detail: sharpening (amount/radius/detail/masking with ⌥-drag mask preview), noise reduction (luminance/detail/contrast, colour/detail/smoothness); a 1:1 detail preview thumbnail.
- Effects: post-crop vignette (style, amount, midpoint, roundness, feather, highlights), grain (amount, size, roughness).
- Crop & Straighten tool in the loupe: aspect presets, free rotate with a straighten-line drag, overlay grids, constrain-to-image; writes the geometry settings.
- Presets (partial recipes as JSON, saved under app support), Snapshots list, History list with step toggles (uses recipe history entries; "Agent" groups later).
Design bar as before. ACCEPTANCE.md Develop section extended. Tests: Swift tests that each panel writes the expected JSON patch and that the curve editor produces monotone points; `cargo test -p tessera-ffi --release`, clippy, fmt, `(cd apps/mac && ./build-ffi.sh && swift build && swift test)`. engine-api unchanged.
