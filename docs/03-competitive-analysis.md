# Competitive Analysis — Photoshop & Lightroom Classic vs. the Field (Sept 2026)

Sources and per-product detail: [research/adobe-current.md](research/adobe-current.md), [research/raster-competitors.md](research/raster-competitors.md), [research/raw-competitors.md](research/raw-competitors.md). Claims below were verified against vendor pages, release notes and trade press on 2026-09-24 unless marked *(unverified)*.

## 1. Adobe baseline (what we are comparing against)

| | Version | Licensing | Notes |
|---|---|---|---|
| Photoshop | 27.10 desktop (Aug 2026), web, iPad, iPhone/Android | Subscription only. Single app ~$23–34/mo; Photography 1TB $19.99/mo; CC Pro $69.99/mo; Mobile & Web $7.99/mo | Generative features metered by credits (≈10 for Firefly, ≈40 for partner models per fill) |
| Lightroom Classic | 15.5.1 (Aug 2026) | Photography plan only | Bimonthly releases; some features now land in ACR/Lightroom cloud first |
| Camera Raw | 18.6 | Bundled | Glow, Trim (ACR-only so far) |
| Lightroom (cloud) | 9.5 desktop / 11.5 mobile | Lightroom 1TB $14.99/mo (raised Mar 2026) | Quick Actions, Generative Expand, natural-language search, Edit using Describe |

Adobe closed several gaps in 2025–26: AI culling with face/eye scoring, duplicate detection, dust removal, reflection removal, non-destructive AI Denoise, Landscape/Snow masks, feather/edge on AI masks, partner generative models, on-device generative Remove, ACR tools as Photoshop adjustment layers. The gaps listed in §4 are the ones that remain.

> Note: sources disagree on whether Generative Expand shipped in Lightroom Classic 15.5 or only in Lightroom desktop/iPhone. Treat it as "cloud-first, Classic uncertain".

## 2. Competitor scorecard

Legend: ● strong · ◐ partial · ○ absent. "Price" is the cheapest way to own/run the full product.

### 2.1 Raw developers / DAM (vs Lightroom Classic)

| Product | Ver. | Price model | RAW quality | DAM | AI masks | Retouch/AI | Tether | Layers | Unique strengths |
|---|---|---|---|---|---|---|---|---|---|
| **Lightroom Classic** | 15.5.1 | Sub $19.99/mo | ● | ● | ● | ● (gen remove, denoise) | ◐ wired | ○ | Ecosystem, sync, plug-ins, modules |
| Capture One Pro | 16.8.6 | Sub ~$18/mo or perpetual | ● (color) | ◐ | ● (people/clothes) | ◐ (offline skin/eyes/teeth) | ● wireless + live cull | ● (opacity) | Sessions, Multi-User Sessions, Live proofing, Match Look, Speed Edit, negative conversion |
| DxO PhotoLab | 10 | Perpetual $249.99 | ● (DeepPRIME XD3, optics modules) | ◐ | ● (depth, per-person parts) | ○ generative | ○ | ○ | Measured lens-softness correction, local denoise, depth masks, U Point, ViewPoint volume correction |
| Luminar | 1.28.1 | Perpetual $129 + Prime sub for AI | ◐ | ○ | ● | ● (sky, relight, body, GenSwap) | ○ | ◐ | Sky AI relighting, Relight/Light Depth, Bokeh 3D, focus stacking, web editor, LrC migration |
| ON1 Photo RAW | 2026 (2027 fall) | Perpetual | ◐ | ◐ (no import) | ● | ● (Tack Sharp, Portrait AI, Keyword AI) | ◐ | ● | Focus stacking, Sky Swap, Keyword AI, Negative Mode, Folder Actions, creative optics |
| darktable | 5.6.1 | Free | ● (choice of demosaic/tone mapper) | ◐ | ◐ (SAM 2.1 local) | ◐ (local denoise/upscale) | ◐ gphoto2 | ◐ (modules) | Every module maskable, scene-referred, Lua, MCP server (5.8 dev), AgX/filmic/sigmoid |
| RawTherapee / ART | 5.13 / 1.26.9 | Free | ● | ○ | ○ (ART external) | ○ | ○ | ○ | Capture sharpening, pixel shift, CIECAM, wavelets, ACES/OCIO/CTL (ART) |
| ACDSee Ultimate | 2026 | Perpetual ~$150 | ◐ | ● (no import, faces) | ◐ | ◐ | ○ | ● | Lightroom+Photoshop in one, Windows only |
| Aftershoot / Imagen / Narrative | — | Sub / per-photo | n/a | ○ | n/a | ● style-learned batch edit + retouch | ○ | ○ | AI profiles trained on your own edits; galleries; culling that learns |
| Radiant Photo | 2 | Perpetual $159 | ○ (rendered files) | ○ | ○ | ● scene-aware auto | ○ | ○ | Best-in-class one-click auto for JPEG volume |
| Photo Mechanic | current | $299 perpetual | n/a | ◐ | ○ | ○ | ○ | ○ | Fastest ingest/caption via embedded JPEGs, code replacements, FTP |
| Zoner Studio | Summer 2026 | $59/yr, Windows | ◐ | ● (no import) | ◐ | ◐ | ○ | ● | Focus stacking, long-exposure sim, moving-object removal, video editor |
| Apple Photos | macOS/iOS 27 | Free | ◐ | ◐ | ○ | ● (Clean Up, Extend, Spatial Reframe) | ○ | ○ | Spatial Reframe (3D re-camera), zero-setup sync |
| Google Photos | — | Free/One | ○ | ◐ | ○ | ● (Help me edit, identity-aware fixes) | ○ | ○ | Conversational editing, Ask Photos NL search |

### 2.2 Pixel editors (vs Photoshop)

| Product | Ver. | Price model | Layers/compositing | Non-destructive | Selections/AI | RAW/HDR/stack | Vector/text/layout | Generative | Unique strengths |
|---|---|---|---|---|---|---|---|---|---|
| **Photoshop** | 27.10 | Sub | ● | ◐ (Smart Objects/Filters) | ● | ◐ | ◐ | ● (Firefly + partner models) | Ecosystem, plug-ins, ACR, AI assistant |
| Affinity (Canva) | 3.3 | Free (AI via Canva Pro) | ● | ● Live Filter layers | ◐ | ● HDR/pano/focus/astro | ● one document | ◐ (Canva) | 32-bit + OCIO 2.5, Tone Map persona, Astro Studio, Scripting Studio, Deform tool, INDD import |
| Pixelmator Pro | 4.0 | $49.99 once / Creator Studio $129/yr | ● | ● | ● (on-device ML) | ◐ | ◐ | ◐ (Apple Intelligence) | iPad/Mac parity, Keynote/Pages/FCP round-trip, ML Deband, warp mockups |
| GIMP | 3.2 | Free | ● | ● GEGL NDE filters, link layers | ○ | ○ | ◐ vector layers | ○ | Linux, Python 3/G'MIC, Perspective Clone, Cage Transform |
| Krita | 5.3/6.0 | Free | ● | ● transform masks | ◐ | ○ | ◐ | ○ | Liquify as non-destructive mask, HDR painting/display incl. Linux Wayland, multi-layer JXL |
| Photopea | continuous | Free/ads, ~$50/yr | ● | ◐ | ◐ | ○ | ◐ | ◐ | Browser, no install, opens PSD/XCF/Sketch/XD/Figma, embeddable |
| Krea | — | Freemium | ○ | n/a | n/a | n/a | ○ | ● real-time, 60+ models | Real-time generative canvas, relight, lens re-render |
| Topaz Photo / Gigapixel | 1.7 / — | Sub only (since Sept 2025) | ○ | n/a | n/a | n/a | n/a | ● restoration | Wonder 3.5 one-shot, Super Focus, Recover Faces, 16× upscale, Dust & Scratch |
| Evoto | 8.0 | Credits | ○ | n/a | n/a | n/a | n/a | ● portrait | Per-face batch retouch, culling, live tethered retouch, video retouch |
| Retouch4me | plugins | Perpetual per plugin | host | n/a | n/a | n/a | n/a | ● portrait | Texture-preserving Heal, auto Dodge & Burn, offline |

## 3. Adobe's durable advantages (why people stay)

- **Raw engine + camera/lens coverage** shared across ACR, LrC and Photoshop; adaptive profiles; AI Denoise/Super Resolution now non-destructive.
- **DAM depth**: catalog scale, hierarchical keywords, smart collections, face recognition, publish services, Map/Book/Print/Slideshow modules, Lua SDK, tethering for five brands.
- **Round-trip and ecosystem**: LrC ↔ Photoshop smart objects, Bridge, Libraries, Fonts, Stock, Express, Firefly Boards; third-party plug-ins and presets.
- **Generative breadth**: Fill/Expand/Remove/Background/Similar/Upscale/Harmonize, model choice (Firefly 5, Gemini/Nano Banana, FLUX.2, OpenAI), AI assistant (beta), on-device Remove.
- **Cross-platform**: web, iPad, iPhone/Android, cloud documents with version history, Content Credentials.

## 4. Features competitors have that Photoshop / Lightroom Classic do NOT (or do noticeably worse)

Grouped by theme, with the implementing competitor and a note on how to build it (cross-referenced to the spec docs).

### 4.1 Licensing & business model
| Feature | Who | Build note |
|---|---|---|
| Free or perpetual full editor | Affinity (free), GIMP, Krita, darktable, RawTherapee (free); Pixelmator ($49.99), DxO, ON1, Luminar, C1, ACDSee, Photo Mechanic (perpetual) | Offer perpetual core + optional metered cloud AI; keep on-device models unmetered |
| AI without credits, fully offline | darktable (SAM 2.1, neural restore), Retouch4me, DxO, Topaz local mode, Pixelmator | Ship ONNX/CoreML models; see [04 §5](04-implementation-architecture.md) |
| Browser editor on a perpetual licence | Luminar Web (Fall 2026), Photopea | WASM build of the shared core |
| Embeddable / white-label editor | Photopea | Expose the WASM editor as an SDK |

### 4.2 Non-destructive editing model (vs Photoshop)
| Feature | Who | Build note |
|---|---|---|
| Live filter layers without Smart Object conversion | Affinity Live Filters, GIMP 3 GEGL filters, Krita filter layers | Make filters first-class layer nodes in the compositor ([02 §1.2–1.3](02-photoshop-spec.md)) |
| Non-destructive transform / liquify / cage / mesh as masks | Krita transform masks, Affinity Deform tool | Store warp fields as layer attributes; re-render on demand |
| Linked external images as layers | GIMP 3.2 Link Layers, Krita file layers | Equivalent to linked smart objects but without conversion step |
| Every raw module maskable with blend modes and multiple instances | darktable | Generalize local adjustments to any operator with a mask + blend + instance list ([01 §2.15](01-lightroom-classic-spec.md)) |
| Real layers inside the raw editor (opacity, blend modes, compositing) | Capture One, ON1, Luminar, ACDSee, Exposure X7 | Add a lightweight layer stack over the recipe pipeline |

### 4.3 Colour pipeline & HDR
| Feature | Who | Build note |
|---|---|---|
| Full 32-bit workflow with OCIO 2.5 and a dedicated tone-map persona | Affinity; Krita (HDR painting, Wayland HDR); ART (ACES CLF/CTL) | Photoshop's 32-bit mode disables many tools; keep all tools float-capable |
| Choice of display transform (filmic / sigmoid / AgX) and CAT-based color calibration | darktable | Pluggable tone mappers in the output stage |
| Choice of demosaic algorithm + capture sharpening (deconvolution) at demosaic | darktable, RawTherapee | Expose demosaic selection; add RL deconvolution stage |
| Pixel-shift multi-frame combining | RawTherapee | Align + merge sub-pixel frames before demosaic |
| CIECAM02/16 colour appearance model, GHS stretch | RawTherapee | Appearance-model based adjustments |
| Colour harmony tool (rotate hues toward harmonic schemes) | darktable colorharmonizer | Hue rotation in a UCS with harmony templates |
| Multi-layer / animated JPEG XL with CICP HDR, multi-layer EXR | Krita, GIMP | Codec support in the image core |
| ML Deband | Pixelmator Pro | Small CNN or dithered smoothing on gradients |

### 4.4 Optics & raw quality (vs Lightroom Classic)
| Feature | Who | Build note |
|---|---|---|
| Lab-measured camera+lens modules including **lens-softness correction** across the field | DxO PhotoLab / PureRAW | Spatially varying deconvolution PSF per lens/aperture/focal; needs a measurement programme |
| Denoise at demosaic stage rated at/above Adobe (DeepPRIME XD3) | DxO | Joint denoise+demosaic network ([01 §2.9](01-lightroom-classic-spec.md)) |
| **Local (brushable) AI denoise and local lens sharpening** | DxO PhotoLab 9/10 | Run denoise globally, blend by mask |
| Genre-specific denoise models (wildlife, astro, portrait, macro) | ON1 NoNoise 2027 | Model selection conditioned on scene class |
| Deblur of mis-focused / motion-blurred shots | Topaz Super Focus, ON1 Tack Sharp | Blind deconvolution + learned deblur network (Photoshop's Shake Reduction is gone) |
| Volume deformation correction for wide-angle faces | DxO ViewPoint | Local sphere-to-plane reprojection near edges |
| Compressed linear DNG (≈4× smaller) | DxO 9.6 | JPEG-XL lossy tiles in DNG 1.7 |

### 4.5 Masking & selection
| Feature | Who | Build note |
|---|---|---|
| Depth-band masks (foreground/mid/background) without embedded depth | DxO PhotoLab 10, Luminar Light Depth | Monocular depth model + range mask ([01 §2.15–2.16](01-lightroom-classic-spec.md)) |
| Per-person masks with finer parts (iris vs pupil vs sclera, facial hair) | DxO 10, Capture One (incl. clothes) | Extend face-parsing classes; instance-separate people |
| U Point control points (similarity from a clicked point, elliptical) | DxO / Nik | Colour+luminance similarity weight around a point |
| Click-to-segment with SAM 2.1 + DenseCRF refinement, offline | darktable 5.6 | SAM-class promptable model on device |
| Mask tool that auto-creates a mask when you paint/gradient | Affinity 3.3 | UX: brush on a layer auto-adds mask |

### 4.6 Retouching & portrait automation
| Feature | Who | Build note |
|---|---|---|
| Identity-aware batch retouch (same person gets the same settings across a shoot) | Evoto | Face embeddings → per-person retouch profiles |
| One-slider skin/face/body reshaping (Body AI, Face AI) | Luminar, Evoto, ON1 Portrait AI, C1 AI retouch | Landmark-driven warps + skin models |
| Texture-preserving automatic Heal and automatic Dodge & Burn that mimic high-end retouchers | Retouch4me | Frequency-separation-aware inpainting; learned D&B maps |
| Glasses-glare and clothing-wrinkle removal as single controls | Evoto, Aftershoot | Targeted inpainting models |
| Live retouch while tethered; video portrait retouch | Evoto Instant / Evoto Video | Real-time inference pipeline on capture stream |
| Recover Faces for low-res/old photos; Dust & Scratch for scans | Topaz | Face-prior restoration GAN; scratch detection + inpaint |

### 4.7 Generative & AI editing UX
| Feature | Who | Build note |
|---|---|---|
| Real-time generative canvas (draw/prompt with live updates), voice mode | Krea | Streaming diffusion (LCM/Turbo) |
| Prompt-based region *replacement* (GenSwap) | Luminar | Masked img2img with prompt (Photoshop Generative Fill covers most of this; LrC has none) |
| Sky replacement with scene relighting and water reflections | Luminar Sky AI, ON1 Sky Swap | Sky seg + horizon + relight net + reflection warp |
| Relight / Studio Light / Atmosphere / fog / sunrays with depth awareness | Luminar, ON1 Depth Lighting | Depth-conditioned relighting |
| Spatial Reframe (3D reconstruction to change camera position) | Apple Photos (iOS/macOS 27) | Gaussian-splat/NeRF from single image + novel view synthesis |
| Identity-aware fixes (open eyes/expression using other photos of the same person) | Google Photos | Person-conditioned generative editing |
| Layer decomposition of a flat image (Magic Grab / Magic Layers / image decomposition) | Canva / Affinity AI | Segmentation + inpainting of occluded regions |
| Layer to 3D | Canva / Affinity AI | Image-to-3D model |
| Camera-lens simulator re-render, relight in edit | Krea Edit | Diffusion conditioned on lens/light params |
| Astrophotography stacking & calibration workflow | Affinity Astro Studio | Dark/flat/bias calibration, star alignment, sigma-clip stacking |

### 4.8 Computational photography & merges
| Feature | Who | Build note |
|---|---|---|
| Focus stacking in the raw/DAM app | ON1, Luminar, Zoner, Affinity | Laplacian-pyramid focus measure ([02 §6](02-photoshop-spec.md)); Photoshop has it, LrC does not |
| Long-exposure simulation and moving-object removal from a burst | Zoner Summer 2026 | Align + mean/median stack |
| HDR merge with a strong tone-mapping persona | Affinity | Local tone mapping operators exposed as a mode |
| Film negative conversion (C-41/B&W inversion with base removal) | Capture One 16.7.4, ON1 Negative Mode | Per-channel inversion with orange-mask estimation |
| Physics-based film simulation (Spektrafilm: grain, halation, diffusion) | darktable 5.8 dev, FilmPack, Exposure X7 | Spectral film model; Adobe has grain + new ACR Glow only |

### 4.9 Studio, tethering, collaboration
| Feature | Who | Build note |
|---|---|---|
| Wireless tethering at near-wired speed | Capture One (Canon) | Camera Wi-Fi SDK + prioritized transfer |
| AI culling on incoming tethered frames during the shoot | Capture One Assisted Review | Run cull models in the ingest pipeline |
| Multi-user real-time editing of one session over LAN with roles | Capture One Studio (beta) | Session change-log sync ([01 §1.13](01-lightroom-classic-spec.md)) over LAN |
| Client review in browser tied to the live session | Capture One Live | Web viewer with selections/comments |
| Sessions (self-contained project folders) as an alternative to a catalog | Capture One | Session = mini-catalog inside a folder |
| Match Look (transfer grade from a reference image) and Smart Adjustments (normalize a set to a reference) | Capture One | Colour transfer + exposure/WB normalization |
| Speed Edit (hold key + scroll to adjust across a selection) | Capture One | Keyboard-modal slider UX |
| Multi-seat culling plans, per-frame Close-Ups face panel | Narrative Select | Face crops grid per frame |

### 4.10 DAM & workflow
| Feature | Who | Build note |
|---|---|---|
| Browse without import (folder mode) | ON1, ACDSee, DxO, Exposure, Photo Mechanic, Zoner, darktable-lite | Ad-hoc folder session that can be promoted to catalog |
| AI keywords written to metadata | ON1 Keyword AI, ACDSee *(unverified)* | Tagging model (CLIP-class) → keyword suggestions with confidence |
| Editing style learned from your own past edits and applied to a whole shoot | Aftershoot, Imagen, Narrative | Train per-user regression on (image → develop settings); ship as native settings |
| Fastest ingest/caption for news (embedded JPEG contact sheets, code replacements, FTP) | Photo Mechanic | Embedded-preview-only browse mode; caption macros |
| Folder Actions automation (auto metadata/presets/export on drop) | ON1 2027 | Watched-folder rule engine |
| Lightroom catalog migration into competitor | Luminar, Capture One | Read `.lrcat` SQLite + XMP |
| Natural-language / visual search in the local catalog | Lightroom cloud only; Google Photos; Apple | Embedding index (CLIP) over previews — do it locally |
| Integrated client galleries with face search and print ordering | Aftershoot | Web gallery service |

### 4.11 Extensibility & platform
| Feature | Who | Build note |
|---|---|---|
| In-app scripting IDE (JavaScript across all studios) | Affinity 3.3 Scripting Studio | Editor + REPL over the scripting DOM ([02 §11](02-photoshop-spec.md)) |
| Headless MCP server so AI agents can drive the raw pipeline | darktable 5.8 dev | Expose the scripting DOM as MCP tools |
| Lua/Python scripting of the pipeline | darktable, GIMP, Krita | Embed interpreter |
| Pixel + vector + page layout in one document; InDesign import | Affinity | Shared document model with vector/layout nodes |
| First-class Linux | GIMP, Krita, darktable, RawTherapee | Vulkan backend + GTK/Qt or custom UI |
| OS-level round-trip (Keynote/Pages/Numbers/Final Cut) and true iPad/Mac parity | Pixelmator Pro | Document provider extensions |
| In-place editing of the system photo library (no import) | Photomator / Apple Photos | PhotoKit-based editing extension |

## 5. Where competitors are weaker (what not to copy)

- Thin DAM: no hierarchical keywords, face recognition, publish services, or print/book modules at LrC depth (Luminar, DxO, C1, ON1, darktable).
- Large-catalog performance and stability (Luminar, ON1); slow exports with heavy denoise (DxO).
- Confusing licensing with expiring AI (Luminar Prime), opaque tiering (ON1), Studio-only gating (C1), subscription pivots that angered users (Topaz, Capture One price rises).
- Fragmented product lines (DxO PhotoLab + PureRAW + Nik + FilmPack + ViewPoint).
- Platform locks: Apple-only (Pixelmator), Windows-only (ACDSee, Zoner), no iPad (Affinity 3).
- Generative quality and control behind Firefly/partner models (Affinity/Canva, Photopea, Pixelmator).
- PSD fidelity limits in every non-Adobe editor (smart objects, some styles).

## 6. Recommended differentiation for a new product

1. **Perpetual/free core, offline AI, metered cloud only for heavy generative work.** This is the market's reference expectation now.
2. **Non-destructive everything**: filter/transform/liquify as layer attributes; every raw operator maskable with blend modes and instances; a light layer stack inside the raw editor.
3. **Pipeline quality levers Adobe lacks**: demosaic choice, capture sharpening, lens-softness deconvolution, local AI denoise, depth-band masks, per-person part masks, tone-mapper choice, OCIO 2.5 float workflow.
4. **Studio collaboration**: sessions, wireless tether with live cull, LAN multi-user, browser proofing.
5. **Personalization**: style profiles trained on the user's own catalog history; AI keywording and local natural-language search.
6. **Computational merges** in the DAM: focus stack, burst long-exposure, moving-object removal, astro stacking, film negative conversion.
7. **Open automation**: scripting DOM, in-app IDE, MCP/agent API, Lua/Python.
8. **Avoid** the DAM gap: ship catalog scale, keywords, faces, publish, print/book from day one, since that is what keeps people on Lightroom Classic.
