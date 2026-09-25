# Image Quality & Colour — State-of-the-Art Requirements for the Develop Engine

This document raises the bar set in [01-lightroom-classic-spec.md §2](01-lightroom-classic-spec.md) from "parity" to "state of the art", and defines how we handle many camera types, optics correction, and colour-correct display.

## 1. Pipeline principles
- Scene-referred, linear, 32-bit float end to end; **no 8/16-bit intermediate anywhere**; dithering only at final encode.
- Deterministic and versioned: recipe + pipeline version ⇒ bit-identical output on any GPU/CPU (we validate against a golden image suite per release).
- Every operator runs on GPU (Metal/Vulkan/D3D12 compute, WebGPU on web) with a CPU reference implementation used for tests.
- Order is fixed but every operator can be **masked, blended and instanced** (darktable-style) rather than only the Lightroom subset.

## 2. Raw decoding & camera support
- Decoder built on LibRaw + rawspeed-class parsers with our own coverage for CR3, HEIF-wrapped raws, Sony lossless compressed, Nikon HE/HE*, Fujifilm X-Trans and GFX, Hasselblad 3FR/FFF, Phase One IIQ, Leica, OM System, Panasonic, Pentax pixel-shift, Sigma Foveon (via DNG), smartphone ProRAW / Expert RAW / Pixel DNG with embedded gain maps and depth.
- Per-camera **calibration bundle** (versioned, downloadable): black/white levels per ISO, noise model (read/shot variance per channel per ISO), colour matrices for two illuminants, spectral-sensitivity-derived profile where measured, default crop, sensor defects, pixel-shift/HDR modes, embedded lens-correction opcode conventions.
- Highlight reconstruction: clip-aware, propagate colour from unclipped channels, then **inpainting-based reconstruction** (guided by neighbouring gradients) for fully clipped areas; user choice: clip / reconstruct colour / reconstruct in LCh / inpaint.
- Demosaic: default **learned demosaic** (CNN trained per CFA family, Bayer and X-Trans) with classical fallbacks selectable (RCD, AMaZE, LMMSE, DCB, Markesteijn, dual-demosaic with variance switching). Pixel-shift multi-frame merge; Fuji/Panasonic/OM high-res modes.
- **Joint denoise + demosaic** network (DxO DeepPRIME-class) as the default for ISO ≥ threshold, conditioned on the camera noise model; runs on packed CFA; Amount slider blends; **local application** via mask (brushable denoise).
- Capture sharpening: Richardson–Lucy deconvolution at the raw stage with lens-specific PSF when available (§3), otherwise estimated Gaussian; radius auto from corner-vs-centre analysis.

## 3. Optics: distortion, chromatic aberration, vignetting, softness
Sources of correction data, in priority order:
1. **Embedded manufacturer opcodes** in the raw (DNG `OpcodeList`, Canon/Sony/Nikon/Fuji/Panasonic/OM embedded distortion & CA & vignette tables). Always parse; many mirrorless lenses only have this.
2. **Our lens profile database**: LCP-compatible format (Brown–Conrady radial k1…k3 + tangential p1, p2, lateral CA per channel, vignetting polynomial) sampled over focal length, aperture and focus distance, plus **PSF/sharpness field** samples for softness deconvolution. Sourced from lensfun (open), our own measurement programme (charts + automated capture), and community-submitted calibrations validated automatically.
3. **Auto-calibration from the image**: if no profile: estimate radial distortion from detected straight lines (LSD → line straightness minimization), estimate lateral CA by per-channel edge alignment across the field (radial scale + higher-order term), estimate vignetting from smooth background gradient statistics. Results can be saved as a user profile for that lens.
4. **Manual**: sliders for distortion, CA, vignette, and defringe with hue ranges.

Implementation notes:
- Lateral CA: warp R and B channels by per-channel radial polynomials **before** demosaic on the CFA plane (avoids colour fringing in interpolation) when profile is exact; otherwise after demosaic.
- Longitudinal (axial) CA / purple fringing: detect saturated purple/green fringes adjacent to high-contrast edges and desaturate toward luminance; ML-based fringe classifier optional.
- Distortion, Upright, crop and user warps compose into **one** inverse map with Lanczos-3 sampling; the map is cached at each zoom level.
- Vignetting applied in linear light before tone; anti-vignette noise amplification compensated by feeding the noise model into denoise.
- Lens softness: spatially varying deconvolution with the profile's PSF field (DxO-style), amount slider, halo guard.
- Coverage target: every lens in lensfun + Adobe LCP coverage equivalent for top 500 lenses by usage; telemetry-driven priority list for new profiles.

## 4. Colour: camera profiling
- Profiles as DCP (dual-illuminant matrices + HueSatMap + LookTable + tone curve) so existing Adobe/third-party DCPs load, plus our extended format adding **spectral sensitivities** when measured and per-ISO deltas.
- Ship: "Neutral" (colorimetric), "Standard" (pleasing, our look), "Camera match" (per-manufacturer film simulations/picture styles reverse-engineered from JPEGs), "Adaptive" (per-image learned).
- Build tools: profile from ColorChecker/IT8 shots (single/dual illuminant) and from spectral data; validation ΔE2000 report.
- White balance in camera space via chromatic adaptation (CAT16) to the working white; presets via correlated colour temperature + Duv; auto WB from a learned illuminant estimator with a gray-world fallback.

## 5. Working space & operators
- Working space: linear Rec.2020 primaries (all real camera colours representable with less negative-value trouble than ProPhoto), float.
- Perceptual operations (HSL, Point Color, Color Grading, Vibrance) in **Oklab/OkLCh** (or JzAzBz for HDR) rather than ad-hoc HSV, so hue is stable under lightness changes.
- Tone: pluggable display transform — our default (a filmic-style sigmoid with per-channel desaturation control and highlight hue preservation), plus AgX, sigmoid, filmic, and an "Adobe PV6 compatibility" mapper used for imported edits.
- Gamut mapping to the output space with perceptual compression (chroma compression toward the achromatic axis, hue-preserving), not clipping; soft-proof shows what compression will do.
- Local contrast (Texture/Clarity/Dehaze) built on edge-aware decompositions (guided filter / local Laplacian pyramids) to avoid halos; dehaze via dark-channel + guided transmission with airlight estimation robust to snow/white scenes.
- Curves: monotone splines with per-channel and luminance-only modes; parametric curves operate on a log-encoded axis so shadows have usable range.

## 6. Display & output colour correctness
- ICC v4 CMM (Little-CMS-class) for display and print; display profiles read from the OS; **built-in calibration workflow** with X-Rite/Calibrite/Datacolor sensors (via ArgyllCMS-class drivers) producing ICC + matrix/LUT profiles; validation patches.
- OS-native colour paths: macOS EDR with Display P3 / Rec.2020 PQ; Windows Advanced Color (scRGB float swapchain) with HDR10 metadata; Linux Wayland colour-management protocol; per-monitor profiles on multi-display; correct handling of the "monitor profile vs document profile" round trip with black-point compensation.
- Rendering intents selectable; soft-proof with paper white/black simulation and gamut warnings for print and for the display itself.
- HDR: unbounded float through the pipeline; SDR and HDR renditions from one recipe; export to AVIF/JXL/HEIF PQ or HLG, JPEG/PNG with ISO 21496-1 gain map, TIFF/EXR float. HDR headroom slider bound to display capabilities reported by the OS.
- 10-bit output surfaces; dithering for 8-bit displays; option to disable OS-level colour transforms for calibrated pro monitors.
- Colour accuracy tests: nightly rendering of ColorChecker captures through each camera profile vs reference measurements; ΔE regression gate.

## 7. Operator inventory upgrades (beyond Lightroom parity)
| Area | SOTA target |
|---|---|
| Denoise | Joint raw denoise+demosaic net, local mask, genre-tuned models, chroma-only mode, temporal (burst) denoise |
| Sharpening | Deconvolution capture sharpening + output sharpening by device; halo-free |
| Upscale | 2×/4× diffusion-free super-resolution net with texture fidelity mode; print-size aware |
| Deblur | Blind PSF estimation + learned non-blind deblur for shake/focus miss |
| Highlights | Inpainting-based highlight reconstruction |
| Geometry | Auto Upright with vanishing-point RANSAC, guided lines, volume deformation correction, content-aware fill of empty corners |
| Local | Any operator maskable; masks: AI (subject, sky, people+parts per person, objects click/box/text prompt, landscape classes, depth bands), geometric, luminance/colour/depth range, edge-aware refinement, feather/edge on all masks |
| Colour | Point Color with variance, 3-way grading, hue/sat/lum curves, colour harmony helper, LUT layer with strength |
| Retouch | Heal/clone/patch, object removal with local inpainting model, distraction/reflection/dust removal, frequency-separation skin tools |
| Merges | HDR (deghost), pano (boundary warp), focus stack, burst long-exposure, moving-object removal, astro stacking |
| Film | Grain (spectral, size/roughness), halation/bloom/diffusion (Glow), negative conversion |
