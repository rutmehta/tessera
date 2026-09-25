# Photoshop — Feature Specification & Implementation Guide

Version baseline: Photoshop 27.x (2026 release train) on desktop, plus Photoshop on the web, iPad, and the Photoshop mobile app (iPhone/Android).
Companion docs: [Lightroom Classic spec](01-lightroom-classic-spec.md) · [Shared engine architecture](04-implementation-architecture.md) · [Competitive analysis](03-competitive-analysis.md)

Template per feature: **What it does** · **Data model** · **Implementation** · **Dependencies**.

Photoshop differs from Lightroom in one fundamental way: it is a *destructive-by-default, layer-based raster compositor* whose non-destructive features (adjustment layers, smart objects, smart filters, layer masks, layer styles) are built on top of a pixel document rather than a recipe. An implementation must therefore be built around a **document model** (Section 1) and a **compositor** (Section 2); every tool is then an operation that mutates or references that model.

---

## 1. Document model

### 1.1 Canvas & image data
- Dimensions up to 300,000 × 300,000 px (Large Document format); color modes Bitmap, Grayscale, Duotone, Indexed, RGB, CMYK, Lab, Multichannel; bit depths 8/16/32 bits per channel; embedded ICC profile; resolution (ppi); pixel aspect ratio; guides, grids, slices, rulers.
- **Implementation.** Tiled storage (e.g., 256×256 tiles) per channel with copy-on-write and per-tile dirty flags; tiles on disk-backed virtual memory ("scratch disks") when RAM is exceeded; a mip pyramid per layer for zoomed-out display. All internal math for 8/16-bit is done in 16-bit ints or float; 32-bit float documents are linear scene-referred with OCIO/ICC view transforms.

### 1.2 Layers
- Types: pixel, adjustment, fill (solid/gradient/pattern), type, shape (vector with fill/stroke), smart object (embedded/linked; raw smart objects run Camera Raw), video/frame layers, 3D (deprecated), group (folder), artboard, frame (placeholder), background (locked).
- Properties: name, color tag, visibility, opacity, fill opacity, blend mode (27 modes across Normal/Darken/Lighten/Contrast/Inversion/Component groups + Pass Through for groups), lock (transparent px / image px / position / artboard nesting / all), clipping mask, layer mask (raster) + vector mask, mask density/feather, "Blend If" sliders per channel with split points, knockout (shallow/deep), blend interior/clipped-as-group, transparency shapes layer, layer comps, linked layers, isolate mode, filter by kind/name/effect/mode/attribute/color/smart object/selected.
- **Data model.** Tree of `Layer` nodes; each has `pixels` (tiled), `mask`, `vectorMask` (paths), `styles` (FX list), `smartFilters` (for smart objects), `blend` params, `transform` (for smart objects: 2D/3D matrix + warp mesh). Document = root group + global state (selection, channels, paths, guides, history).

### 1.3 Smart Objects & Smart Filters
- Embed or link an external file/document; non-destructive transform (scale/rotate/warp/perspective); duplicate as linked instances; "Convert to Layers"; replace contents; stack modes (mean/median/…); filters applied become Smart Filters with individual masks, opacity/blend, reordering, on/off.
- **Implementation.** Smart object = child document + cached rendered proxy at document resolution; edits re-render the child then re-apply transform + filter stack. Filter stack is an ordered list of (filter id, parameters, blend options); rendering is cached until any upstream change. Linked SOs watch file mtime.

### 1.4 Layer Styles (Effects)
- Bevel & Emboss (+Contour, Texture), Stroke, Inner Shadow, Inner Glow, Satin, Color/Gradient/Pattern Overlay, Outer Glow, Drop Shadow; multiple instances of Stroke/Overlay/Shadow; global light; scale effects; styles presets library; copy/paste; convert to layers.
- **Implementation.** Each effect computed from the layer's alpha (and, for Satin/Bevel, from a distance field or blurred alpha): shadows = offset + Gaussian-blurred alpha × color; glows = dilated/blurred alpha with contour LUT and jitter; bevel = shading of a height field derived from blurred alpha (with contour curve and optional texture); stroke = distance-field threshold. Composite in the documented order (styles render beneath/above fill by type).

### 1.5 Channels, Paths, Selections
- Channels: color + alpha channels + spot color channels; Quick Mask mode; save/load selections; "Apply Image"/"Calculations".
- Paths: Bézier paths with work path / saved paths / shape paths / vector masks; clipping paths for print.
- Selection: an 8-bit alpha mask (soft edges), plus marching ants render, "Select and Mask" workspace, Transform Selection, Modify (border/smooth/expand/contract/feather), Grow/Similar, Refine Edge Brush, Color Range, Focus Area, Sky, Subject, all-layer sampling.
- **Implementation.** Selection = tiled 8-bit mask; marching ants via contour extraction at ≥50% threshold; Boolean ops (add/subtract/intersect) are per-pixel max/min. Paths → mask via anti-aliased scanline rasterization.

### 1.6 History, Snapshots, Non-linear history, History Brush
- History states (configurable count), snapshots, history brush (paint from a chosen state), art history brush, "Allow Non-Linear History", purge.
- **Implementation.** Each state = a set of changed tiles (copy-on-write) and a model diff; memory-bounded ring; history brush samples from the source state's tiles.

### 1.7 File formats
- Native PSD/PSB (with maximize compatibility flattened composite), TIFF (layers), PDF, JPEG, JPEG 2000, PNG, GIF, WebP, AVIF, HEIF, JPEG XL (via plugin), BMP, TGA, Radiance HDR, OpenEXR, DICOM, raw via Camera Raw, SVG import, Illustrator paste as smart object, video (via timeline), 3D (deprecated). Cloud documents (`.psdc`) with version history and offline availability.
- **Implementation.** PSD reader/writer must handle: image resources block, layer & mask info with extra data keys (`lspf`, `luni`, `lrFX`/`lfx2`, `SoLd`/`SoLE` for smart objects, `vmsk`, `TySh`, `shmd`, `lclr`, `fxrp`), RLE and zip compression, 16/32-bit variants, PSB 64-bit lengths, and adjustment layer keys (`levl`, `curv`, `hue2`, `brit`, `blnc`, `expA`, `vibA`, `phfl`, `mixr`, `clrL`, `grdm`, `selc`, `blwh`, `post`, `thrs`, `nvrt`). Use the published Adobe Photoshop File Format spec.

---

## 2. Compositor & color management

**What it does.** Renders the layer tree to the display and to flattened output with correct blend modes, group pass-through, clipping masks, knockouts, Blend If, layer styles, masks (density/feather), and smart-object transforms; supports GPU compositing, zoom levels, rotate view, birds-eye, multiple windows per document.

**Implementation.**
- Tile-parallel compositing on GPU (Metal / DirectX 12 / Vulkan) with CPU fallback; composite bottom-up, groups rendered to intermediate buffers unless Pass Through; adjustment layers apply their transform to the current composite below (respecting clipping and mask).
- Blend math per Adobe/PDF specs (Multiply, Screen, Overlay, Soft/Hard/Vivid/Linear/Pin Light, Hard Mix, Difference, Exclusion, Subtract, Divide, Hue/Sat/Color/Luminosity, Darker/Lighter Color, Dissolve with random alpha). Fill opacity affects only the interior, not styles.
- Color management: ICC v2/v4 via a CMM; working spaces per mode; document profile; "Proof Setup" and gamut warning; "Blend RGB colors using gamma 1.0" option; 32-bit float documents use linear light with a view LUT (OCIO config support for VFX).
- Display: Wide-gamut (P3) and HDR (EDR/HDR10) display paths; "Precise" vs "Fast" color rendering; 30-bit display support.

---

## 3. Selection & masking tools

| Tool | What it does | Implementation |
|---|---|---|
| Rectangular / Elliptical / Single row/col Marquee | Geometric selections, feather, anti-alias, fixed ratio/size | Rasterize shape to alpha with AA |
| Lasso / Polygonal / Magnetic Lasso | Freehand; polygon; edge-snapping | Magnetic: live-wire (Dijkstra on gradient cost) between anchor points |
| Object Selection | Hover to highlight detected objects, click to select; box/lasso mode; "Object Finder" | Instance segmentation (Mask R-CNN / Mask2Former-class) precomputed on open; hover shows instance mask; refine with guided filter |
| Quick Selection | Brush that grows to edges | Region growing on color/texture with edge stopping; incremental graph cut |
| Selection Brush | Paint a selection like a mask with configurable overlay | Direct mask painting with brush engine |
| Magic Wand | Tolerance-based flood fill (contiguous/all layers) | Flood fill in color distance with tolerance |
| Select Subject | One-click subject, cloud or device model; "People" priority | Salient object segmentation + person detection; cloud model for higher quality |
| Select Sky | Selects sky regions | Sky semantic segmentation |
| Select > Focus Area | In-focus regions | Local sharpness (Laplacian variance) map + graph cut with color |
| Color Range | Sampled colors, skin tones, detect faces, highlights/mid/shadows, out-of-gamut, fuzziness/range | Distance in Lab with fuzziness falloff; skin tone = learned classifier |
| Select and Mask workspace | View modes (onion skin, overlay, on black/white, B&W, layers), Refine Edge Brush, global refinements (smooth, feather, contrast, shift edge), Decontaminate Colors, output to selection/mask/new layer, "Refine Hair", object-aware mode | Alpha matting (closed-form / deep matting network) in the brushed trimap region; color decontamination = re-estimate foreground colors using matting equation |
| Quick Mask | Paint selection as color overlay | Selection mask displayed as tinted overlay |
| Modify / Grow / Similar / Transform Selection | Morphology & similarity | Morphological ops; Similar = wand across whole image |
| Save/Load Selection, Channels | Alpha channel persistence | Alpha channel storage in document |

---

## 4. Painting, brush engine & fill tools

**What it does.** Brush tool with the full brush engine: tip shape (sampled or computed, size, hardness, spacing, angle, roundness, flip), shape dynamics (size/angle/roundness jitter with pen pressure/tilt/rotation/wheel), scattering, texture, dual brush, color dynamics, transfer (opacity/flow jitter, build-up), brush pose, noise, wet edges, airbrush, smoothing (with pulled string, catch-up, stroke end, adjust for zoom), symmetry painting (vertical/horizontal/dual axis/diagonal/wavy/circle/spiral/parallel lines/radial/mandala), pattern brushes, erodible tips, bristle tips (legacy), Mixer Brush (wet/load/mix/flow, clean/dirty, sample all layers), Pencil, Color Replacement, Eraser (background eraser, magic eraser), Gradient tool (non-destructive gradient layers with on-canvas editing; linear/radial/angle/reflected/diamond; dither; classic vs perceptual/linear interpolation), Paint Bucket, Pattern Fill/Stamp, Content-Aware Fill workspace, Brush presets & tool presets, Brush Settings panel, Paint Symmetry, Live Brush Tip Preview, Frame tool, Color panel/HUD picker, Swatches/Libraries.

**Implementation.**
- Stamp-based brush engine: stroke = sequence of input events (x, y, pressure, tilt, rotation, velocity, timestamp) → smoothed via a pulled-string / Catmull-Rom filter → dabs placed at `spacing × size` intervals along the path with per-dab parameters from dynamics (jitter, pressure curves) → dab rendered as a tip (raster or procedural) with texture modulation → accumulated into a stroke buffer with **flow** (per-dab) then composited with **opacity** (per-stroke max) into the layer. GPU rasterization per dab with tile-dirtying.
- Mixer brush: maintain a "reservoir" color and per-dab pickup from canvas; mixing in RGB with wet/load/mix parameters; bristle model optional.
- Symmetry: transform each input event through the symmetry group and replay.
- Gradient: parametric fill layer with stops (color + opacity), interpolation in perceptual (Oklab), linear, or classic sRGB; dither to avoid banding in 8-bit.
- Content-Aware Fill: PatchMatch-based synthesis with sampling area mask, color adaptation (none/default/high/very high = gradient-domain blending), rotation adaptation, scale, mirror; output to new layer.

---

## 5. Retouching tools

| Tool | What it does | Implementation |
|---|---|---|
| Spot Healing Brush | Auto-source healing; modes Content-Aware / Create Texture / Proximity Match; sample all layers | Find source via PatchMatch in a neighborhood, blend with gradient-domain (Poisson) healing |
| Healing Brush | Alt-sample source; heal blends texture from source with color/luminance of destination | Copy source texture, Poisson blend into destination boundary conditions |
| Patch tool | Drag selection to source; Normal/Content-Aware modes with Structure/Color | Same as CAF with an explicit source region |
| Content-Aware Move / Extend | Move object and fill hole; extend | Cut-paste + CAF on the hole + gradient blending at the seam |
| Remove tool | Brush over an object; auto-detects and removes people/objects (cloud or device); "Remove after each stroke" | Inpainting network (LaMa-class / diffusion-lite) on a dilated mask; optional distraction removal (people/wires detection) |
| Distraction Removal | One-click remove people, wires & cables | Detection network → mask → Remove |
| Clone Stamp | Sample & paint; clone source panel (5 sources, offset, scale, rotate, overlay) | Direct sampling with transform; aligned/non-aligned |
| Red Eye | Click to fix | Red-blob detection + desaturate |
| Dodge / Burn / Sponge | Range (shadows/mid/highlights), exposure, protect tones | Exposure-weighted curve per range; Sponge = chroma scale |
| Blur / Sharpen / Smudge | Brush-local filters; smudge with finger painting | Local convolution; smudge drags a color sample along the stroke |
| Liquify | Forward warp, reconstruct, smooth, twirl, pucker, bloat, push left, freeze/thaw mask, **Face-Aware Liquify** (eyes, nose, mouth, face shape per detected face), mesh save/load, smart-object non-destructive | Displacement mesh (vector field) edited by brushes; face landmarks (68/106-point) drive parametric warps; render by inverse mapping with bilinear/bicubic sampling |
| Neural Filters | Skin Smoothing, Smart Portrait (age/expression/gaze), Super Zoom, JPEG Artifact Removal, Colorize, Style Transfer, Harmonization, Landscape Mixer, Depth Blur, Photo Restoration, Makeup Transfer, Color Transfer; some cloud-only | Per-filter neural models (device via ONNX/CoreML, or cloud); output to new layer/smart filter |
| Camera Raw Filter | Full ACR panel as a filter (see Lightroom spec §2) | Embed the shared raw engine operating on RGB pixels (no demosaic stage) |
| Adjustment Brush (new) | Paint an adjustment (exposure, etc.) creating a masked adjustment layer | Brush → mask + adjustment layer |

---

## 6. Transform & geometry

- **Free Transform**: scale, rotate, skew, distort, perspective, warp (custom mesh, presets: arc, flag, wave, fish, etc., with split control), flip, reference point, interpolation (nearest/bilinear/bicubic/smoother/sharper/automatic), "Content-Aware Scale" (seam carving with protect mask / skin protection), **Perspective Warp** (define quad planes, then warp), **Puppet Warp** (mesh + pins, density, expansion, rotation), Smart object non-destructive transforms, Image Size (resample methods incl. Preserve Details 2.0 with noise reduction), Canvas Size, Image Rotation, Crop tool (ratio presets, content-aware crop fill, straighten, overlays, delete cropped px toggle, classic mode), Perspective Crop, Trim, Reveal All, Auto-Align Layers, Auto-Blend Layers (panorama seamless tones / focus stack), Photomerge (Auto/Perspective/Cylindrical/Spherical/Collage/Reposition + vignette/distortion removal + content-aware fill transparent), Lens Correction filter (profile-based + custom), Adaptive Wide Angle (constraint lines on fisheye/wide), Vanishing Point (perspective planes for clone/paint).

**Implementation.**
- Transform = affine/homography applied via inverse mapping; warp = Bézier mesh (4×4 by default, subdividable) with inverse lookup via Newton iteration or pre-rasterized displacement field. Puppet Warp = as-rigid-as-possible (ARAP) mesh deformation over a triangulated alpha shape. Perspective Warp = user quads → per-quad homographies with continuity constraints, then bilinear blend across quads.
- Content-Aware Scale = seam carving with energy = gradient magnitude + protect mask (+ skin detector).
- Preserve Details 2.0 = learned upscaler (CNN) + noise reduction; Bicubic variants classic.
- Auto-Align = feature matching (SIFT-class) + RANSAC homography/cylindrical model per layer; Auto-Blend = seam finding (graph cut) + multiband blending (panorama) or Laplacian-pyramid focus measure selection (stack images).
- Adaptive Wide Angle = model camera (fisheye/rectilinear from lens profile) → sphere → reprojection with constraint lines solved as a least-squares warp.
- Vanishing Point = user-defined perspective planes (homographies); clone/paint operations are applied in plane coordinates and mapped back.

---

## 7. Adjustment layers & Image > Adjustments

| Adjustment | Controls | Implementation |
|---|---|---|
| Brightness/Contrast | ±150 brightness, ±100 contrast, legacy toggle | Non-legacy uses a tone-preserving curve around midtones |
| Levels | Input black/gamma/white, output black/white per channel, auto options, eyedroppers, presets | LUT: `out = ((in − ib)/(iw − ib))^(1/γ) × (ow − ob) + ob` |
| Curves | RGB + channels, point/pencil, show clipping, auto (Enhance Monochromatic/Per Channel Contrast/Brightness&Contrast/Find Dark&Light), TAT | Monotone spline → 256/65536-entry LUT per channel |
| Exposure | Exposure, Offset, Gamma (32-bit oriented) | Linear-light gain/offset/gamma |
| Vibrance | Vibrance, Saturation | See LR §2.2 |
| Hue/Saturation | Master + 6 ranges with adjustable band widths, Colorize | Hue-band weight functions in HSL |
| Color Balance | Shadows/Mid/Highlights CMY-RGB sliders, preserve luminosity | Tone-weighted RGB offsets |
| Black & White | 6 color sliders, tint, presets, auto, TAT | Weighted luminance per hue band |
| Photo Filter | Filter presets/color, density, preserve luminosity | Multiply with color, luminance-preserving |
| Channel Mixer | 3×3 matrix + constant, monochrome | Matrix multiply |
| Color Lookup | 3D LUT (.cube/.3dl/.look), abstract & device link profiles | Trilinear/tetrahedral 3D LUT interpolation |
| Invert / Posterize / Threshold | — | Per-pixel LUT |
| Gradient Map | Gradient with dither, reverse, method (perceptual/linear/classic) | Luminance → gradient lookup |
| Selective Color | CMYK offsets per 9 color ranges, relative/absolute | Per-color-range membership × CMYK correction |
| Shadows/Highlights | Amount, tone, radius per range; color, midtone, black/white clip | Edge-aware local tone mapping (bilateral base) |
| HDR Toning | Local adaptation, equalize histogram, exposure & gamma, highlight compression | Tone mapping operators for 32-bit |
| Desaturate / Match Color / Replace Color / Equalize / Auto Tone/Contrast/Color | Various | Match Color = mean/std transfer in Lab (Reinhard); Replace Color = Color Range + HSL shift |

Every adjustment layer supports a mask, blend mode, opacity, clipping, and the Properties panel; presets saved per adjustment.

---

## 8. Filters

- **Filter Gallery** (Artistic, Brush Strokes, Distort, Sketch, Stylize, Texture with stacking), **Blur** (Average, Blur, Blur More, Box, Gaussian, Lens Blur (with depth map alpha), Motion, Radial, Shape, Smart, Surface), **Blur Gallery** (Field, Iris, Tilt-Shift, Path, Spin blur with bokeh & motion effects; non-destructive on smart objects), **Distort** (Displace, Pinch, Polar, Ripple, Shear, Spherize, Twirl, Wave, ZigZag), **Noise** (Add Noise, Despeckle, Dust & Scratches, Median, Reduce Noise), **Pixelate** (Color Halftone, Crystallize, Facet, Fragment, Mezzotint, Mosaic, Pointillize), **Render** (Flame, Picture Frame, Tree, Clouds, Difference Clouds, Fibers, Lens Flare, Lighting Effects), **Sharpen** (Sharpen, Edges, More, Smart Sharpen with lens/gaussian/motion removal + shadow/highlight fade, Unsharp Mask, Shake Reduction), **Stylize** (Diffuse, Emboss, Extrude, Find Edges, Oil Paint, Solarize, Tiles, Trace Contour, Wind), **Video** (De-Interlace, NTSC), **Other** (Custom kernel, High Pass, HSB/HSL, Maximum, Minimum, Offset), Camera Raw Filter, Liquify, Vanishing Point, Neural Filters, Lens Correction, Adaptive Wide Angle, Generative Fill (as filter surface), third-party filter plug-ins (8bf), Smart Filters with filter masks.

**Implementation.**
- Separable Gaussian, box, and motion blurs on GPU; Lens Blur = depth-slice or scatter/gather with aperture shape and specular boost; Blur Gallery = spatially varying kernels driven by pin geometry (field = interpolated pins; iris = elliptical falloff; tilt-shift = band; path = along-path motion vectors; spin = angular blur).
- Smart Sharpen = deconvolution (Richardson–Lucy/Wiener) with the chosen PSF; Shake Reduction = blind PSF estimation (multi-region), then non-blind deconvolution with artifact suppression.
- Reduce Noise = per-channel wavelet shrinkage with "preserve details" and JPEG artifact removal (deblocking).
- Oil Paint = Kuwahara-style anisotropic smoothing + relief shading; Render filters = procedural noise (Perlin) and parametric generators.
- 8bf plug-in host: implement the Photoshop Plug-in SDK filter interface (buffer/host callbacks); modern extensibility via UXP.

---

## 9. Type & vector

- **Type**: point/paragraph/path/shape-bound text; Character panel (font, size, leading, kerning/tracking, scale, baseline, color, faux styles, OpenType features, stylistic sets, ligatures, discretionary, fractions, ordinals, swashes, contextual alternates); Paragraph (alignment, justification, indents, hyphenation, composer, spacing); variable fonts; SVG color fonts & emoji; Adobe Fonts activation; Match Font (image → font suggestion); text warp; convert to shape/path/work path; Paragraph & Character styles; text on a path; Live text preview; Glyphs panel; Type > Rasterize.
- **Vector**: Pen (standard, freeform, curvature, content-aware tracing), shapes (rect with per-corner radii, ellipse, polygon/star, line, custom shape library), live shape properties, path operations (combine/subtract/intersect/exclude), stroke options (dashes, caps, joins, alignment), gradient/pattern fills, export to SVG, paste from Illustrator as shape/smart object/path, Align/Distribute.
- **Implementation.** Text via a shaping engine (HarfBuzz) + line breaker (Knuth–Plass for the paragraph composer) + rasterization (FreeType/CoreText/DirectWrite with hinting off, AA); vectors rasterized with scanline AA into the layer at document resolution and re-rasterized on transform (resolution independent). Text warp = mesh warp of the rasterized glyph outlines (applied to outlines, not pixels, to stay vector).

---

## 10. Generative AI (Firefly) features

| Feature | What it does | Implementation |
|---|---|---|
| Generative Fill | Select area → optional prompt → 3+ variations on a new layer with mask; reference image support; model picker (Firefly Image 3/4, partner models e.g. Google/OpenAI/FLUX where offered) | Diffusion inpainting: send crop (≤ 2048 px) + mask + prompt to a service; upscale/blend result into the document; commercial-safe model. Content Credentials tag |
| Generative Expand | Extend canvas and outpaint | Outpainting = inpainting with the expanded region masked |
| Generative Remove (Remove tool cloud mode) | Remove objects with prompt-free inpainting | As above without prompt; distractor-aware |
| Generate Background | Isolate subject, replace background from prompt | Subject mask + background outpaint with subject conditioning |
| Generate Similar / Generate Image | Variations of an object; text-to-image into a layer | Image-to-image with IP-adapter-style conditioning; T2I |
| Harmonize | Match a composited layer's color/light/shadow to the scene | Relighting/harmonization network conditioned on background |
| Generative Upscale | AI upscale up to 8 MP+ (or 4× within limits) | Super-resolution diffusion/GAN |
| Reference Image | Guide style/composition | Style conditioning |
| Generative Workspace | Batch ideation of images/prompts outside the document | Separate gallery UI over the same service |
| Text to Vector / Text to Pattern (Illustrator features exposed via Libraries) | — | — |
| AI Assistant (conversational/agentic editing) | Natural-language multi-step edits ("remove the background, add a warm tone, export at 2000px") executed as Photoshop actions | LLM-driven planner that maps intents to Photoshop's scripting DOM (UXP/ExtendScript API), executes steps, and shows an editable action list |

Generative features are metered by *generative credits* on the account; results carry C2PA credentials on export.

### 10.1 Current state (Sept 2026) and additional items to implement

Dated changelog with sources: [research/adobe-current.md](research/adobe-current.md).

| Feature (version) | Implementation pointer |
|---|---|
| Partner models in Generative Fill/Generate Image: Gemini 2.5/3.1 (Nano Banana / Nano Banana 2 / Pro), FLUX.1 Kontext, FLUX.2 pro, OpenAI; Firefly Image 5; model picker; credit-cost preview on hover; Credits Usage panel (27.0–27.8) | Model-agnostic generation gateway with per-model cost metadata |
| Firefly Fill & Expand model at 2K output (Jan 2026) | Tile-and-blend for >2K regions |
| Generative Upscale via Topaz Gigapixel/Bloom + Firefly Upscaler up to 4K (27.0) | Pluggable upscalers |
| Harmonize GA (27.0), Rotate Object (27.6, 3D-like re-angling of a 2D object, no credits) | Harmonize: relighting net; Rotate Object: single-image 3D lifting + novel-view synthesis |
| Reflection Removal (27.6), outputs reflection to a separate layer with opacity (27.8) | Reflection/transmission separation network → two layers |
| Remove tool: Find Distractions mode; on-device offline generative removal (27.8) | Local inpainting model (LaMa/diffusion-lite) |
| ACR tools as adjustment layers: Color & Vibrance with Temp/Tint (27.0), Clarity, Dehaze, Grain (27.3), Light = Exposure/Contrast/Highlights/Shadows/Whites/Blacks (27.10) | Wrap raw-engine operators as adjustment-layer nodes ([01 §2.2](01-lightroom-classic-spec.md)) |
| Select Details (hair, clothing, facial parts) (26.6); improved Select Subject/Remove Background cloud or on-device (27.0) | Part-parsing segmentation |
| AI Assistant public beta on web/mobile (Mar 2026); desktop AI Assisted Editor beta, Prompt-to-Edit, Markup-guided generation, masks/selections constrain generative edits (27.10); cross-app Firefly AI Assistant | Planner over scripting DOM; markup → conditioning masks/arrows |
| AI Layer Cleanup, Actions panel with natural-language search (27.6) | Layer-content classifier for naming; embedding search over action names |
| Dynamic Text: auto-fit, arc/circle, any path/shape (26.8–27.10) | Text engine with fit-to-frame and path layout |
| Firefly Boards integration, Projects, Adobe Stock panel, Express templates (26.11–27.10) | Cloud service integrations |
| AVIF and JPEG XL with HDR (26.8); OCIO/ACES colour management and better 32-bit (26.0) | Codec + OCIO in image core |
| Substance 3D materials/viewer (beta), Generative Workspace (beta), Live Co-Editing (private beta since Jan 2025) | See §14–15 |
| Neural Filters: shipping but stagnant since 2024 | Keep as a plug-in surface |


---

## 11. Automation, extensibility & scripting

- **Actions** panel (record/play, conditional actions, insert menu item/stop/path, button mode, batch across folder with file naming, droplets); **Scripts** (JavaScript via UXP; legacy ExtendScript/AppleScript/VBScript; Script Events Manager; Image Processor; Load Files into Stack; Statistics; Layer Comps to Files; Export Layers to Files; Fit Image; Lens Correction batch; Contact Sheet II; Photomerge); **Plug-ins** (UXP panels & commands from the Creative Cloud marketplace; legacy 8bf filters; 8be export plug-ins; Generator for asset export); **Variables & Data Sets** (data-driven graphics); **Tool presets, Workspaces, Keyboard shortcuts, Menus customization**; **Creative Cloud Libraries**; **Adobe Bridge / Camera Raw** integration; **Adobe Express** integration for quick tasks.
- **Implementation.** Expose the document model through a scripting DOM (Application → Documents → Layers → …) with an action-descriptor (key/value) layer underneath that every UI command routes through, so recording an action == capturing descriptors. Plug-in host runs UXP (JS + HTML panels in a sandbox) with an API surface over the DOM.

---

## 12. Export, output & print

- Export As (PNG/JPG/GIF/SVG/WebP, scale, resample, metadata, color space convert to sRGB, per-layer/artboard export with suffixes), Quick Export, Save for Web (legacy, GIF optimization, slices), Generator image assets, Save a Copy, Cloud documents, Share for Review/comment links, Artboards to PDF, Print (color management managed by Photoshop/printer, proof, bleed, registration marks, calibration bars, 16-bit output, scale to fit), Contact sheet, PDF presentation, Zoomify, Print Studio.
- **Implementation.** Flatten via compositor at target resolution → convert to output profile → encoder with quality/chroma-subsampling/metadata choices; SVG export walks vector layers; artboard iteration for batch.

---

## 13. Camera Raw (as part of Photoshop)

Camera Raw is the same engine as Lightroom's Develop module (see [01-lightroom-classic-spec.md §2](01-lightroom-classic-spec.md)). Photoshop-specific behaviors: opens raw into a Smart Object (editable later), Camera Raw Filter for rasters (no demosaic stage), workflow options (color space, depth, size, sharpen for, open as smart object), snapshots inside ACR, ACR presets synchronized with Lightroom, "Open in Photoshop as Layers" and "Merge to HDR/Panorama" from Bridge/Lightroom.

---

## 14. Video, animation, 3D (legacy)

- Timeline (video layers, clips, transitions, audio, keyframed position/opacity/style/transform for layers, frame animation mode, onion skinning, render video via Adobe Media Encoder presets, image sequence import/export).
- 3D features are deprecated (removed from current builds); Substance 3D materials via plug-in.
- **Implementation.** Video layers decode frames on demand; keyframes interpolate layer properties per frame; render loop composites each frame through the same compositor.

---

## 15. Collaboration, cloud & cross-platform

- Cloud documents with version history, invite to edit, comments, share-for-review links, **Live Co-Editing** (multi-user simultaneous editing, beta), Photoshop on the web (browser version with core tools + generative features), Photoshop on iPad (near-full), Photoshop mobile app (iPhone/Android, layered editing with generative tools, free tier), Creative Cloud Libraries, Adobe Fonts, Content Credentials, Behance/Stock integration.
- **Implementation.** Cloud docs = chunked, deduplicated PSD storage with delta sync; co-editing = operational-transform/CRDT over the document model at the operation (action descriptor) level with layer-level locking; web/iPad = shared C++ core compiled to WebAssembly (with OPFS storage) and native, respectively, with the same document model and compositor.

---

## 16. Workspace, UI & accessibility

- Customizable panels/workspaces (Essentials, 3D, Graphic & Web, Motion, Painting, Photography), Contextual Task Bar (suggests next actions: select subject → mask → generative fill), Discover panel (search/tutorials/quick actions), Properties panel (context-aware: layer, adjustment, type, document, Quick Actions like Remove Background / Select Subject), Home screen, Recent files, Preferences (performance: GPU, cache levels/tile size, history states, scratch disks; interface; tools; cursors; transparency & gamut; units; guides; plug-ins; type; enhanced controls), Touch bar/gestures, Scrubby zoom, Bird's-eye, Rotate view, Screen modes, Navigator, Info, Histogram, Notes, Timeline, Measurement, Color, Swatches, Gradients, Patterns, Shapes, Styles, Brushes, Brush Settings, Clone Source, Character, Paragraph, Glyphs, Layer Comps, Comments, Version History, Libraries, Learn, Actions, Adjustments, Channels, Paths, Layers.

---

## 17. Implementation priority matrix (suggested)

| Tier | Features |
|---|---|
| P0 | Tiled document model, PSD I/O, layer compositor with blend modes/masks/groups, selection mask, brush engine (basic), transforms, core adjustment layers (Levels, Curves, HSL, B&W, Color Balance), Gaussian/USM/High Pass filters, Type (basic), export, history |
| P1 | Smart objects & smart filters, layer styles, vector shapes/paths/pen, Select and Mask with matting, Content-Aware Fill/heal/patch, Liquify, Puppet/Perspective warp, Camera Raw filter (shared engine), Blur Gallery, Smart Sharpen, Actions & scripting DOM, color management with soft proof |
| P2 | Object Selection / Select Subject / Sky / Remove tool (on-device models), Neural filters, Face-Aware Liquify, Content-Aware Scale, Photomerge / Auto-Align / Auto-Blend, Adaptive Wide Angle, Vanishing Point, Video timeline, UXP plug-in host |
| P3 | Generative Fill/Expand/Background/Similar/Harmonize/Upscale (service-backed), AI assistant, cloud documents & co-editing, web/iPad/mobile builds, Content Credentials |
