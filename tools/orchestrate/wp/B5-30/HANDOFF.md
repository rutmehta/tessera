# B5-30 handoff — the document canvas colour-manages non-sRGB documents (option 0: tagging)

Branch `wp/B5-30` from `origin/main` `1d74771b`. **Option 0 chosen** (tag, don't convert): no engine pixel path,
present.wgsl, render.rs quantize, format or export change. Rust: one small FFI getter
(`crates/tessera-ffi/src/document/io.rs`) and `stack::same_profile` made `pub(super)`. The rest is Swift.

## What changes on screen
The backend still writes the document's own ENCODED samples into the RGBA8 surfaces. What changes is the colour
space those samples are declared to be in:

| document | layer colour space | surface sampled as | composite space | EDR flag |
|---|---|---|---|---|
| untagged, or the built-in sRGB (any creation date) | extended linear sRGB (unchanged) | `rgba8Unorm_srgb` (unchanged) | linear sRGB | on (unchanged) |
| any other RGB profile CoreGraphics reads (P3, Adobe RGB, ProPhoto, Rec.2020, files) | **the document profile** | `rgba8Unorm` (no decode) | the profile's own encoding | off |
| profile CoreGraphics rejects, or non-RGB | sRGB path, plus a status-bar/NSLog diagnostic | | | |

Core Animation / WindowServer converts the layer from its colour space to the display. Zero per-pixel cost in the
app: the shader is unchanged (it only drops the hardware sRGB decode for non-sRGB documents). The checkerboard and
canvas background colours are converted into the document profile for that layer, so they look the same as before.
The ring IOSurfaces are also tagged (`kIOSurfaceColorSpace` = the profile's ICC; metadata only). The detail panes
(filter sheet, Camera Raw, Liquify, Adaptive Wide Angle previews) build their CGImage in the same space.

## Why option 0 works, and the EDR finding (question (a))
- Tagging the IOSurface alone would NOT have worked: the viewport never hands the IOSurface to Core Animation. It
  wraps it in an `MTLTexture` and samples it in a shader that draws into the layer's RGBA16F drawable; Metal
  sampling ignores IOSurface colour tags. The colour space that matters is the `CAMetalLayer.colorspace`.
- The layer does not need to stay extended-linear. EDR headroom is never used by the canvas: surfaces are 8-bit
  (values ≤ 1), theme colours are SDR, and `setDisplayHeadroom` is not called by the app. The extended linear sRGB
  layer exists only because `rgba8Unorm_srgb` decodes the sRGB curve in hardware, so the shader output is linear sRGB.
- The "linearized twin" alternative (extended-linearized document space + hardware decode) is only correct for
  profiles whose TRC is the sRGB curve (Display P3 yes; Adobe RGB 2.2, ProPhoto 1.8, Rec.2020 no) and would need a
  shader TRC table for the rest. Tagging the layer with the document profile itself and sampling without decode is
  exact for every profile CoreGraphics accepts, so that is what non-sRGB documents use. sRGB documents keep the old
  configuration byte for byte, so nothing changes for them.
- One deliberate difference: semi-transparent canvas pixels are blended over the checkerboard in the document's
  encoding for non-sRGB documents (Photoshop also composites transparency for display in document space). Opaque
  pixels are exact.

## Other checks
- (b) untagged = sRGB: `display_profile_icc()` is `None` for untagged documents and for the built-in sRGB
  (`same_profile`, ignoring creation date and profile ID); the viewport keeps the sRGB path.
- (c) unusable profiles: `CGColorSpace(iccData:)` nil, or not 3-channel RGB → sRGB, with the diagnostic
  "Display: the colour profile “X” cannot be read by macOS / is not an RGB profile; the canvas shows it as sRGB"
  (status bar via `DocumentController.report` when set, always NSLog). Never crashes. A non-embedded profile (only a
  handle, no bytes) is shown as sRGB, as before: the built-ins are generated with the current date, so a handle cannot
  be matched reliably.
- (d) export unaffected: no export code touched (export still converts via ICC in the engine).
- (e) zero per-pixel cost: the profile bytes are read over FFI only when the document is attached and when
  `info.profileName` changes (open, Assign/Convert to Profile, undo across them); no pixel work anywhere.

## API
- Rust: `DocumentSession::display_profile_icc() -> Result<Option<Vec<u8>>>` (`document/io.rs`, pure helper
  `display_icc`). Bindings regenerated (`TesseraFFI.swift`, `CTesseraFFI.h`).
- Swift `TesseraCore`: `DocumentDisplayColor` (`resolve(icc:name:)`, `space`, `isSRGB`, `diagnostic`, `iccData`,
  `tag(_:)`); `DocumentBackend.displayProfileICC()` (protocol requirement with a `nil` default; engine adapter
  implements it).
- Swift app: `DocumentController.displayColor`; `DocumentViewportView.displayColorDidChange()` (called on attach and
  on profile change; re-tags layer + ring, rebuilds ring textures when the format flips);
  `FilterSheetModel.image(_:width:height:space:)` (new required `space:`); Liquify/AWA `image(_:_:)` take the
  colour.

## Tests
RED `47df62dd` (stubs: getter returns `None`, `resolve` always sRGB). Failing lines on RED:
- Rust `a_p3_document_hands_over_its_own_icc_bytes`: `left: None, right: Some([...P3 ICC...])` (io.rs:971).
- Swift `DocumentDisplayColorTests.swift:93` P3 document: `XCTUnwrap failed … a P3 document hands over its ICC bytes`.
- Swift `:124, :128, :131, :132` unsupported/garbage/grey profiles not falling back with a diagnostic; P3 ICC not
  resolved (`3144 bytes` sRGB != `536 bytes` P3).
- The sRGB test passed on RED by design (it pins the unchanged sRGB path).
After RED the Rust test was adjusted (not weakened): the built-in sRGB of another creation date must still be sRGB,
and a non-embedded P3 is sRGB (handle matching against date-stamped built-ins is not reliable).
Tests: sRGB document → no ICC, pane bytes `[255,0,0,255]`, pane CGImage sRGB, layer extended linear sRGB with EDR,
`rgba8Unorm_srgb`; P3 document → ICC bytes == profile, pane bytes identical to the sRGB document's
(`[255,0,0,255]`: tagging, not converting), pane CGImage ICC == profile, layer colour space ICC == profile,
`rgba8Unorm`, every ring IOSurface tagged with the profile, re-attaching an sRGB document restores the sRGB path;
unsupported (garbage bytes, grey profile) → sRGB with a diagnostic naming the profile.

## On-screen check for A (B to run; needs a wide-gamut / P3 display, e.g. any recent MacBook Pro / Studio Display)
1. `swift tools/orchestrate/wp/B5-30/make-fixtures.swift /tmp` writes `/tmp/b5-30-red-p3.png` and
   `/tmp/b5-30-red-srgb.png`: identical pixel bytes (left half 255,0,0; right half grey 128), one embedding Display
   P3, one embedding sRGB (`sips -g profile` confirms).
2. Open both in Tessera as documents (File ▸ Open), side by side or by switching tabs, 100 % zoom.
3. Expected: the P3 document's red is visibly more saturated / deeper than the sRGB one's; the grey halves match;
   Info shows "Display P3" vs "sRGB IEC61966-2.1". Before B5-30 both reds looked identical.
4. Optional: Filter ▸ Camera Raw Filter… on each: the 1:1 detail pane red matches its canvas.
5. Optional: Digital Color Meter set to "Display native values" over each red: the P3 one reads ~255,0,0 on a P3
   panel; the sRGB one reads ~235,51,35.
Note the Apple sRGB ICC in the sRGB fixture is not byte-identical to the engine's built-in sRGB, so that document
takes the document-profile path; it must still look like sRGB (that also exercises the tagged path for sRGB).

## Gates
See the commit message / report: cargo test --release -p tessera-ffi, clippy -p tessera-ffi --all-targets
-D warnings, fmt --check, build-ffi.sh, tools/orchestrate/swift-gate.sh.
