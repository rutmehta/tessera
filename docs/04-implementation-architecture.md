# Shared Engine Architecture — How to Build the Lightroom + Photoshop Feature Set

This document describes one codebase that can back both a Lightroom-class raw workflow app and a Photoshop-class layered editor. It is referenced by the per-feature implementation notes in [01-lightroom-classic-spec.md](01-lightroom-classic-spec.md) and [02-photoshop-spec.md](02-photoshop-spec.md).

## 1. High-level components

```
┌──────────────────────────────────────────────────────────────────────┐
│ Apps: Raw/DAM app (LrC-class) · Layered editor (Ps-class) · Mobile/Web│
├──────────────────────────────────────────────────────────────────────┤
│ UI toolkit (native: SwiftUI/AppKit, WinUI; web: WASM + Canvas/WebGPU)│
├───────────────┬───────────────┬───────────────┬──────────────────────┤
│ Catalog/DAM   │ Raw Develop   │ Layer Compositor│ ML Runtime           │
│ (SQLite, XMP) │ (recipe-based)│ (tiled, GPU)   │ (CoreML/DirectML/ONNX│
│               │               │                │  + cloud gateway)    │
├───────────────┴───────────────┴───────────────┴──────────────────────┤
│ Image core: tiles, pyramids, color mgmt (ICC/OCIO), codecs, geometry │
├──────────────────────────────────────────────────────────────────────┤
│ GPU abstraction (Metal / D3D12 / Vulkan / WebGPU) · Job scheduler    │
└──────────────────────────────────────────────────────────────────────┘
```

Language: C++20 (or Rust) core with a thin platform layer; ML via ONNX Runtime with execution providers (CoreML, DirectML, CUDA, WebGPU/WASM).

## 2. Image core

- **Tiles**: 256×256 or 512×512, float32 (raw pipeline) or uint16/uint8 (layer documents), planar per channel, halo of 8–32 px for neighborhood ops. Copy-on-write tile references make undo, history, virtual copies and smart-object caching cheap.
- **Pyramids**: 2× mip chain per image/layer for zoomed-out rendering and for multi-scale algorithms (guided filter bases, Laplacian pyramids for blending, wavelets for NR).
- **Color**: working spaces — raw: linear ProPhoto/RIMM float; layered docs: document ICC profile at 8/16-bit or linear float at 32-bit. CMM (Little-CMS class) for ICC v2/v4 with BPC, intents, soft-proof; OCIO for 32-bit VFX workflows. Display path supports SDR, wide gamut (P3), HDR (PQ/HLG via EDR/HDR swapchains). Adobe-compatible **gain-map** HDR export (ISO 21496-1).
- **Codecs**: raw decode (LibRaw-class with per-vendor parsers incl. CR3/HEIF-wrapped, RAF X-Trans, DNG 1.7 incl. JPEG-XL compressed), DNG write, JPEG/JPEG XL/AVIF/HEIF/WebP/PNG/TIFF/PSD/PSB/EXR/HDR, video via OS frameworks.
- **Geometry**: a single resampling stage that composes lens distortion + CA + Upright/transform + crop + user warp into one inverse map (bicubic/Lanczos sampling); avoids cumulative blur.

## 3. Raw Develop engine (recipe-based)

- **Recipe** = ordered map of parameters (versioned "process version"), serialized as XMP (`crs:` namespace) for interoperability with Adobe/other tools. Masks and healing stored procedurally with cached rasters.
- **Pipeline graph**: fixed stage order (decode → linearize → denoise(AI) → demosaic → lens → camera profile → WB → detail → tone → color → locals → effects → geometry → output). Each stage is a GPU kernel over tiles; outputs memoized keyed by `(imageId, stageId, hash(params up to this stage), tileCoord, level)`.
- **Interactivity**: render at screen resolution first (from a cached mid-pipeline buffer), then refine to 1:1 tiles under the viewport; slider drags update only downstream stages.
- **Camera support**: DCP profiles (Adobe Standard/Color-equivalents per body), LCP lens profiles, noise profiles per body/ISO. Support user-created DCPs from color-checker shots.

## 4. Layer compositor

- Scene graph of layers; each layer has a render function producing tiles; blend nodes implement Photoshop's blend modes + Blend If + knockout; groups render to intermediate targets when not pass-through; adjustment layers are functions of the composite below.
- Caching: per-layer rendered tiles at each pyramid level; invalidation propagates up the tree from the edited node.
- Selection/masks are 8-bit or float single-channel tiled images.
- Smart objects embed a nested document rendered with its own compositor and cached at the parent's resolution with transform.

## 5. ML runtime & model catalog

| Capability | Model class | Runs |
|---|---|---|
| Subject/background, sky, objects (promptable), people & parts, landscape classes | Semantic/instance/promptable segmentation (U²-Net, Mask2Former, SAM-class) | device |
| Face detection/landmarks/embeddings | RetinaFace + ArcFace-class | device |
| AI denoise (raw), Raw Details demosaic, Super Resolution | CNN on packed Bayer / linear RGB | device |
| Depth estimation (Lens Blur, Depth Blur) | Monocular depth (DPT-class) | device |
| Distraction/reflection removal, inpainting (Remove tool) | LaMa-class inpainting; reflection separation nets | device or cloud |
| Auto tone/color, Adaptive profiles | Small regression nets + predicted 3D LUT | device |
| Generative Fill/Expand/Background/Upscale/Harmonize | Diffusion models (service) | cloud, metered |
| Culling (focus/eyes/duplicates), keyword suggestion | Classifiers + embeddings | device |
| Assistant | LLM planner over scripting DOM | cloud |

Models are versioned; masks/depth maps store the model version so results can be regenerated or kept stable.

## 6. Catalog / DAM

SQLite (WAL), FTS5, closure tables for keyword hierarchies, faceted metadata counts, smart-collection rule compiler, XMP sidecar sync, preview stores (pyramidal JPEG) and Smart Previews (lossy DNG), background job scheduler with priorities (UI > previews > exports), file-system move/rename transactions, sync change-log for cloud.

## 7. Extensibility

- Scripting DOM + action descriptors (records every UI command) → Actions, batch, droplets.
- Plug-ins: UXP-style JS panels in a sandbox; Lua-style SDK for DAM/publish services; native filter plug-in ABI for legacy compatibility.
- Presets as partial recipes (XMP), profiles (DCP + LUT), brushes (ABR-compatible), LUTs (.cube/.3dl), styles (ASL).

## 8. Performance targets (reference)

- Interactive slider latency < 16 ms at screen resolution on a 45 MP raw (GPU).
- 1:1 tile render < 100 ms for the viewport region.
- Export 45 MP JPEG < 1.5 s per image on a modern laptop GPU; parallel across images.
- AI Denoise 45 MP < 5 s on Apple M-series / RTX-class GPU.
- Catalog: 1M+ images with grid scroll at 60 fps (virtualized, thumbnails cached).

## 9. Suggested team/module breakdown

1. Image core & GPU (tiles, color, codecs, geometry)
2. Raw pipeline (decode, demosaic, profiles, tone/color operators, locals)
3. Compositor & tools (layers, brush engine, selections, transforms, filters)
4. ML (segmentation, denoise/SR, depth, inpainting, auto-settings; model ops & cloud gateway)
5. DAM (catalog, import, metadata, previews, sync, publish)
6. Output (export, print, book/slideshow, HDR formats, Content Credentials/C2PA)
7. Platform & extensibility (UI toolkit, scripting DOM, plug-in hosts, web/mobile builds)
