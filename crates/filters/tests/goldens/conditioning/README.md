These synthetic Camera Raw/Develop references contain 259 x 17 interleaved
RGBA f32 samples, little endian, in the named encoded document profile.
The suffix is filter opacity (1, 0.35, or 0). Source pixels and settings are
constructed by `tests/camera_raw.rs`.

Provenance, exact before/after hashes, per-pixel attribution, support proof,
and the non-writing regeneration audit are in
`tools/orchestrate/wp/ENG-1/conditioning/` at the repository root.

Re-recorded 2026-11-05 after PL-1 (3bafea17, 2026-10-16) recalibrated the
native Contrast, Shadows, Highlights and Vibrance responses the fixture
recipe uses: every opacity-1 and 0.35 reference moved by up to 0.17
encoded (a uniform tone shift), the opacity-0 references are unchanged.
The live check against the Develop render in `tests/camera_raw.rs`
(`GOLDEN_TOLERANCE`) still attributes the filter to the renderer.
