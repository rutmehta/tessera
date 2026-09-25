# AI Features — Full Inventory

All non-generative models run **on device, offline, unmetered**. Generative (diffusion) features are a later phase and are metered only if served from the cloud. Each feature lists the model class, where it plugs into the engine, and the user-facing surface. See [04-implementation-architecture.md §5](04-implementation-architecture.md) for runtime.

## 1. Understanding (runs automatically in the background at index time)
| Feature | Model | Output | Surface |
|---|---|---|---|
| Face detection + landmarks | RetinaFace/SCRFD-class + 106-pt landmarks | boxes, landmarks, pose | People view, culling face panel, per-person masks |
| Face recognition / clustering | ArcFace-class embeddings + HDBSCAN | person clusters, suggestions | Name people, auto-tag, search "photos of X" |
| Object & scene detection | Open-vocabulary detector (OWL/Grounding-DINO-class) + scene classifier | labels with confidence | Auto keywords (opt-in write to XMP), filters, search |
| Image embeddings | CLIP/SigLIP-class | vectors | Natural-language search, similar images, style clustering |
| Duplicate / burst grouping | Perceptual hash + embedding + capture-time | groups | Auto-stack, culling groups |
| Quality scores | Focus (per-face and global sharpness), eyes open/closed, blink, motion blur, exposure/clipping, noise, subject emotion, composition/aesthetic score | per-image and per-face scores | Culling ([06](06-culling-and-selection.md)) |
| Depth estimation | Monocular depth (DPT/Depth-Anything-class) | depth map | Depth masks, lens blur, relighting |
| Segmentation | Semantic (sky, landscape classes, water, ground…), instance (people, objects), part parsing (skin, hair, eyes, lips, teeth, clothes) | masks | AI masks, retouch targets |
| Promptable segmentation | SAM-class (click/box/text) | mask | Objects mask, selection |
| OCR / text detection | OCR model | text | Search inside images (signs, bibs, slates) |
| Horizon / vanishing lines | Line + VP detection | geometry | Auto Upright, auto straighten |
| Auto keywords & captions | VLM captioner | text | Keyword suggestions, alt-text export |

## 2. Editing (invoked by user or agent)
| Feature | Model / method | Notes |
|---|---|---|
| Auto settings (tone, colour, WB) | Regression net → slider values | Explainable: outputs sliders, not pixels |
| Adaptive profiles / adaptive presets | Per-image predicted LUT + spatial mask | Lightroom Adaptive Color equivalent |
| Style profiles ("edit like me") | Per-user model trained on the user's own recipes ([10](10-agentic-editing.md)) | Aftershoot/Imagen-class |
| Denoise, demosaic, super-resolution, deblur | See [07](07-image-quality-and-color.md) | Local (masked) application |
| Object/distraction removal | LaMa-class inpainting on device; distraction detector (people, wires, dust, reflections) | Non-generative; results cached as pixel patches |
| Reflection removal | Reflection/transmission separation | Outputs reflection as a separate layer |
| Portrait retouch | Skin smoothing with texture preservation (frequency-aware), blemish detection + heal, eye/teeth enhancement, stray hair, glasses-glare removal, red-eye | Per-person settings persist across a shoot via face identity |
| Face/body reshaping | Landmark-driven mesh warps | Explicit, reversible, with guardrails |
| Relight / studio light | Depth + normal estimation → shading | Non-generative relighting |
| Sky enhancement | Sky mask + graded adjustments | Sky *replacement* is a generative-phase item |
| Lens blur / bokeh | Depth-driven layered blur | Aperture shapes, cat-eye |
| Colour matching | Reference-image colour transfer (statistics in Oklab) | Match Look / Smart Adjustments equivalent |
| Auto crop / composition suggestions | Saliency + aesthetic scorer | Suggest crops; never auto-apply |
| Upright / straighten | VP detection | Auto and guided |

## 3. Workflow AI
- **Culling**: scores, groups, "best of burst", learning from user decisions ([06](06-culling-and-selection.md)).
- **Search**: natural-language + faceted, local; "photos of Anna laughing at the beach, 2024, 85 mm".
- **Keywording**: suggestions with confidence; bulk accept; hierarchical mapping to the user's keyword tree.
- **Duplicates & near-duplicates** across the library.
- **Agentic editing**: see [10-agentic-editing.md](10-agentic-editing.md).

## 4. Generative (phase 2, optional)
Generative fill/expand/replace, sky replacement, background generation, generative upscale beyond 4×, text-to-image. Design constraints: runs through the same recipe model (results are pixel-edit patches with provenance), Content Credentials attached on export, model choice pluggable (local diffusion where hardware allows, cloud otherwise), costs shown before running. Recommendation: ship phase 1 first; add generative *removal fallback* (when local inpainting fails on large holes) and *expand* as the first generative features, since they extend existing tools rather than adding a new surface.

## 5. Model operations
- Versioned model registry; per-feature model version stored in recipes so results are reproducible.
- Hardware tiers: fp16/int8 variants; graceful degradation to smaller models on low-end GPUs; CPU fallback for correctness.
- Privacy: nothing leaves the machine unless the user enables a cloud feature; local telemetry opt-in only.
- Evaluation: per-feature benchmark sets (segmentation IoU, culling agreement with human picks, denoise PSNR/SSIM on paired sets, ΔE for colour models) gated in CI.
