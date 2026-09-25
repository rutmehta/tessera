# Lightroom Classic Competitors: RAW Developers, DAM and AI Workflow Tools
*Research date: 2026-09-24. Verified against vendor pages, release notes and trade press where possible. Items marked **(unverified)** come from background knowledge and were not re-confirmed in this pass.*

---

## 0. Baseline: what Lightroom Classic has as of Sept 2026 (so we don't over-claim gaps)

LrC 15.x (15.0 Oct 2025 through 15.5 Aug 2026) added:
- **Assisted Culling** (subject/eye focus scoring, keep/reject flags, sensitivity), **Faces panel** with Eye Focus and Eyes Open scores (15.4), **auto duplicate detection** into stacks (15.4), **Auto Stack** by time or visual similarity (15.0)
- Remove tool that auto-handles shadows and reflections, **AI dust removal**, reflection removal, Generative Remove, **Generative Expand** (with "Flatten AI edits"), Denoise, Super Resolution
- Point Color with Variance, Snow landscape mask, AI masks now with **Feather and Edge** sliders (15.5)
- Render to DNG (15.5), HDR editing and headroom slider, 4K slideshow export, keyword sync across the Lightroom ecosystem (15.4), Leica tethering

Sources: [Lightroom Queen 15.5](https://www.lightroomqueen.com/whats-new-in-lightroom-2026-08/), [PhotoshopCAFE LrC 15](https://photoshopcafe.com/lightroom-classic-2026-new-features-lightroom-15/), [Adobe 15.4 announcement](https://community.adobe.com/announcements-673/lightroom-classic-v15-4-is-live-cull-people-shots-faster-auto-detect-duplicates-sync-keywords-everywhere-1627070)

**Implication:** Basic AI culling, generative remove/expand, AI dust removal, mask feathering and AI denoise are now parity features. The gaps below are the ones that remain.

---

## 1. Summary: highest-value gaps in Lightroom Classic

| Gap in LrC | Who does it | Notes |
|---|---|---|
| **Real layers / compositing** in the RAW editor | Capture One, ON1, Luminar, ACDSee Ultimate, Exposure X7 | LrC has masks but no layer stack, no layer opacity/blend modes, no multi-image composite |
| **Studio tethering depth**: wireless tethering, sessions, multi-user editing, overlays, Live client review | Capture One | 2nd Gen Wireless Tethering (Canon, near-wired speed), Multi-User Sessions on LAN (Studio tier), Capture One Live, Sessions |
| **AI culling during the shoot** (tethered) | Capture One | Assisted Review runs automatically on incoming tethered frames |
| **Lab-measured optical corrections + deep-learning demosaic/denoise at RAW conversion** | DxO PhotoLab / PureRAW | Camera+lens pair modules, lens softness correction, DeepPRIME XD3 now for Bayer too; **local** DeepPRIME and local lens sharpening |
| **Depth-based masking** | DxO PhotoLab 10, Luminar (Light Depth, Bokeh AI 3D), ON1 (Depth Lighting) | Mask by distance band without depth data |
| **Fine-grained face-part segmentation** (iris, pupil, sclera, teeth, facial hair, lips) per-person | DxO PhotoLab 10, Capture One (People masking, clothes masking, retouch eyes/teeth) | LrC has Select People sub-parts, but DxO/C1 go further and treat individuals separately |
| **Personalized AI editing trained on your own past edits** | Aftershoot, Imagen, Narrative | Style profile learned from 2,000-5,000 edited images; applied to whole shoots |
| **AI retouching as batch presets** (skin, glare on glasses, clothes, backdrops) | Aftershoot, Capture One (AI retouch), Luminar (Skin/Face/Body AI), ON1 Portrait AI | LrC has no automated skin retouch |
| **Sky replacement / relighting / atmosphere** | Luminar, ON1 | Sky AI, Relight AI, Atmosphere 2.0, Sky Swap AI |
| **Focus stacking** (and in-app HDR/pano/stacking variety) | ON1, Luminar, Zoner, darktable (HDR auto-align in 5.8 dev) | LrC merges HDR/pano only |
| **Film negative conversion** | Capture One (16.7.4), ON1 (Negative Mode) | LrC relies on third-party plugins |
| **AI keyword generation** into metadata | ON1 Keyword AI, ACDSee **(unverified: AI keywords)** | LrC has no written auto-keywords |
| **Browse without import / catalog-free** | ON1 Browse, Exposure X7, ACDSee, Photo Mechanic, DxO PhotoLab | LrC requires import into a catalog |
| **Ingest/caption speed for news/sports** | Photo Mechanic | Embedded-JPEG preview, code replacements, IPTC templates |
| **Browser-based editor with perpetual license** | Luminar (Web, Fall 2026) | Lightroom web exists but only on subscription |
| **Perpetual licensing** | DxO, ON1, Luminar, Capture One, ACDSee, Radiant, Exposure, Photo Mechanic | Adobe is subscription-only |
| **Open, scriptable pipeline / agent API** | darktable (Lua, and **MCP server** in 5.8 dev), ART (CTL/OCIO) | darktable is building a headless MCP binary so AI agents can drive the RAW pipeline |
| **Scene-referred wide-gamut pipeline with choice of tone mappers** | darktable (filmic, sigmoid, AgX), RawTherapee/ART | LrC has one fixed process version |
| **Lightroom library migration into competitor** | Luminar (Lightroom Migration beta, 1.27.1), Capture One (catalog importer) | Signals switching friction is being attacked |

---

## 2. Skylum Luminar (formerly Luminar Neo)

**Version / status:** Luminar **1.28.1** (Sept 16, 2026). The "Neo" suffix was dropped with 1.28.0 (Aug 5, 2026), which merged Catalog and Edit into one workspace with a new right-edge toolbar (Presets / Tools / Edits / Extra / Info). Fall 2026 upgrade (pre-order) adds Crop 2.0, Atmosphere 2.0 (fog), Enhance 2.0, AI Assistant 2.0 and **Luminar Web** (browser editor).

**Pricing (Sept 2026):** Desktop Only $129 (perpetual desktop + 1 year of AI tools); All Platforms $159 (adds perpetual mobile + web, 3 devices each); Max $199 (adds 1 year of **Luminar Prime**, renews $59/yr). Generative tools (GenErase, GenSwap, GenExpand), Restoration and AI Assistant need an active Prime subscription after year one. Sky AI, Light Depth and Background Removal stay permanently. Luminar X membership $39/yr for assets. Platforms: macOS, Windows, iOS/Android, Web.

**Feature inventory**
- RAW: own RAW engine; 1.28.1 switched to Adobe DNG SDK for DNG and fixed Apple ProRAW. Historically weaker RAW detail/demosaic than LrC.
- AI tools: Enhance AI, Sky AI, Relight AI, Structure AI, Atmosphere AI, Sunrays, Magic Light, Light Depth, Studio Light, Bokeh AI (now 3D depth model, Spring 2026), Portrait Bokeh, Face AI, Skin AI (blemish/shine removal, dark circles), Body AI, Mask AI + Object Select (now with feather and edge shift)
- Extensions: GenErase, GenSwap, GenExpand, Supersharp AI, Noiseless AI, Upscale AI, HDR Merge, Panorama Stitching, Focus Stacking, Restoration, Background Removal
- DAM: folder-based catalog, albums, ratings/flags; minimal keywording/metadata
- New: **Lightroom Migration (beta)** in 1.27.1 moves an LrC library into Luminar
- Layers: image and adjustment layers with blend modes (texture/overlay compositing)
- Mobile/Web sync; AI Assistant text-prompt editing guidance

**What it does that LrC doesn't (or does worse)**
- **Sky replacement with automatic relighting** of the scene (Sky AI), plus reflections in water. LrC has no sky replacement.
- **Relight AI / Light Depth / Studio Light**: relights foreground vs background using a depth estimate; LrC has only 2D masks.
- **Bokeh AI with a 3D depth model** plus Portrait Bokeh. LrC Lens Blur is comparable in concept; Luminar's is more interactive.
- **Body AI** (slimming/shape) and one-slider skin/face retouch; LrC has no automated portrait retouch.
- **GenSwap**: prompt-based replacement of a region with generated content. LrC has Remove/Expand but no prompt-driven replace.
- **Focus stacking** in-app.
- **Atmosphere / fog / sunrays** generated with depth awareness.
- **Browser editor under a perpetual license** (Fall 2026).
- **Layers** for compositing textures/overlays.

**Weaknesses vs LrC:** Weak DAM (no robust keywording, hierarchical keywords, smart collections, publish services, map/book/print modules comparable to LrC); historically slow and less stable with large libraries; RAW fidelity and lens-profile coverage behind Adobe; confusing licensing with features that expire after a year unless you buy Prime; limited tethering (none).

Sources: [Skylum what's new](https://skylum.com/whats-new/luminar-neo), [Skylum pricing](https://skylum.com/luminar/pricing), [Kathrin Federer on rename/fall roadmap](https://en.kathrinfederer.ch/post/luminar-update-2026-luminar-neo-is-now-called-luminar-and-what-else-is-coming-summer-fall), [Photofocus Spring 2026](https://photofocus.com/news/luminar-spring-upgrade-2026-smarter-ai-better-portrait-tools-a-more-seamless-workflow/), [Photo Rumors fall pre-order](https://photorumors.com/2026/08/06/the-major-new-luminar-prime-fall-upgrade-2026-is-now-available-for-pre-order-with-a-50-off-early-bird-price/)

---

## 3. Capture One Pro / All-in-One / Studio

**Version:** **16.8.6** desktop (Sept 16, 2026); Capture One mobile 4.x. Continuous-release model (no more annual numbered versions).

**Pricing (after 6% increase June 2, 2026):** Pro ~ $18/mo annual (~$27.50 month-to-month); All-in-One ~ $25/mo annual; Studio ~ $48.50/mo annual (~$62.50 monthly). Perpetual licenses still sold (price also raised 6%; exact figure not published in sources checked). Platforms: macOS, Windows, iPad, iPhone. Criticism: Pro alone now costs more than Adobe's Photography Plan.

**Feature inventory**
- RAW: highly regarded color science and per-camera profiles; Enhanced Denoise (16.8, May 2026, preserves skin texture), also on mobile 4.0
- Color: Color Editor (advanced, skin tone uniformity), Color Balance (3-way), Levels/Curves per channel, **Match Look** (drag in any reference image, transfer grade), **Smart Adjustments** (normalize exposure/WB across a set to a reference)
- Local: **Layers** (adjustment and fill layers with opacity), AI Masking (Subject, Background, AI Select), **People Masking** incl. clothes, Combine Masks, luma range masks, gradient/radial, AI Erase
- Retouch: AI retouching (blemish removal, even skin, retouch eyes and teeth, include neck), offline
- Workflow: **Sessions** and Catalogs, **Speed Edit** (hold key + scroll to adjust multiple images), keyboard-centric Cull view, **Assisted Review (beta)** flags closed eyes, missed focus, black frames/exposure problems, now running automatically during tethering (16.8.6)
- Tethering: best-in-class wired tethering for Canon, Nikon, Sony, Fujifilm, Leica, Phase One etc.; **2nd Gen Wireless Tethering** (near-wired speed, Canon incl. R3/R6 III; iOS wireless tethering too); Live View, overlays, next capture naming/adjustments, auto-crop for tethered orientation
- Collaboration: **Capture One Live** (client review in browser), **Multi-User Sessions (beta, Studio, macOS)**: several operators editing the same session over local network in real time with role-based permissions
- Studio: Session Builder, Contact Sheets, Studio ICC profiles for e-commerce, Content Credentials, Background Replacement (via Photoroom API, All-in-One/Studio), **Negative Film Conversion** (16.7.4)
- DAM: catalogs with keywords, albums, smart albums; Keywords on mobile; cloud settings sync

**What it does that LrC doesn't (or does worse)**
- **Wireless tethering at near-wired speed** and broadest camera tethering; LrC tethering is wired and more limited/less reliable.
- **Multi-User Sessions**: real-time collaborative editing of one session on a LAN. Nothing comparable in LrC (single-user catalog, no network use).
- **AI culling on live tethered capture** (Assisted Review during tethering).
- **Sessions** (self-contained project folders with Capture/Selects/Output/Trash).
- **Layers** with per-layer opacity, and more capable Color Editor/skin-tone uniformity.
- **Match Look** and **Smart Adjustments** to normalize a set against a reference image. LrC's Auto Sync / Match Total Exposures is cruder.
- **Speed Edit** modifier-key editing across a selection.
- **Native negative film conversion.**
- **Capture One Live** client proofing tied into the session.
- **AI skin/eye/teeth retouch**, offline.

**Weaknesses vs LrC:** Price now above Adobe's plan; no generative fill/expand; weaker DAM at scale (keyword tooling, face recognition, map, book, publish services); smaller plugin/preset ecosystem; steeper learning curve; many studio features gated to Studio tier; Multi-User macOS-only beta.

Sources: [Capture One what's new](https://www.captureone.com/en/explore-features/whats-new), [DPReview 16.8.6](https://www.dpreview.com/news/capture-ones-latest-update-includes-automatic-culling-and-more-canon-support/), [PetaPixel Multi-User Sessions](https://petapixel.com/2026/06/25/capture-one-adds-real-time-multi-user-sessions-for-studio-pros/), [Multi-User Sessions support](https://support.captureone.com/hc/en-us/articles/36870089862813-Multi-User-Sessions-Beta), [PetaPixel price increase](https://petapixel.com/2026/05/27/capture-one-to-increase-all-product-prices-by-6/), [Newsshooter pricing critique](https://www.newsshooter.com/2026/06/01/capture-one-raises-prices-again-is-it-still-worth-it-or-time-to-jump-ship/), [Alex on RAW 16.7](https://alexonraw.com/capture-one-16-7-combine-masks-better-retouching-and-more/), [Fstoppers AI masking](https://fstoppers.com/capture-one/ai-masking-beats-lightroom-649850)

---

## 4. DxO PhotoLab 10 (+ PureRAW 6, Nik Collection 9, FilmPack 8, ViewPoint 5)

**Version:** **PhotoLab 10**, released Sept 1, 2026 (9.6 in March 2026 brought DeepPRIME XD3 to Bayer sensors). Perpetual: $249.99 new, $129.99 upgrade from v8/v9. macOS and Windows. 30-day full trial.

**Feature inventory**
- RAW: **DeepPRIME / DeepPRIME XD2s / XD3** deep-learning demosaic + denoise applied during RAW conversion; XD3 now for Bayer and X-Trans
- **Optics Modules**: lab-measured camera body + lens pair corrections (distortion, vignetting, CA, and **lens softness/sharpness correction** across the field)
- **Local DeepPRIME and local Lens Sharpness Optimization** (PhotoLab 9): denoise/sharpen only where needed
- ClearView Plus (dehaze/local contrast), Smart Lighting (spot-weighted), Selective Tone
- Local: **U Point control points** (now elliptical with rotation), Control Brush integrating U Point with brushing, graduated/radial filters, luminosity masks, **AI Masks** (subject/sky/people), feather/diffusion for AI masks (9.6)
- **PhotoLab 10:** **Depth Masks** (AI depth map, target foreground/midground/background bands with feathering, no embedded depth needed); AI people masks treat **each person separately**; face sub-part segmentation (iris, pupil, sclera, teeth, facial hair, skin, lips); **single-wheel color grading** (from Nik 9); **AI Sensor Dust Removal**; "Crop as Shot"; round-trip to Affinity
- Export: Linear DNG with **high-fidelity compression** up to ~4x smaller (9.6)
- DAM: PhotoLibrary (folder browsing, projects, keywords, search), no import step needed
- Add-ons: **FilmPack 8** (film emulation, TimeMachine), **ViewPoint 5** (perspective, volume deformation correction for wide-angle faces, keystoning), **Nik Collection 9** (Apr 2026; Color Efex, Silver Efex, Viveza, etc.), **PureRAW 6** (Feb 2026: standalone pre-processor for LrC users; XD3 for Bayer, compressed DNG, AI dust removal, parallel batch)

**What it does that LrC doesn't (or does worse)**
- **Optical correction by measured camera+lens pairs, including sharpness restoration of lens softness.** LrC profiles fix distortion/vignetting/CA but not lens softness per field position.
- **Denoise at demosaic stage (DeepPRIME XD3)**, widely rated at or above Adobe Denoise for detail at high ISO; X-Trans rendering notably cleaner.
- **Local denoise and local lens sharpening.** LrC Denoise is global.
- **Depth Masks** (distance-band masking).
- **Per-person AI masks and finer facial parts** (iris vs pupil vs sclera, facial hair).
- **U Point** control points, which select by similarity from a clicked point.
- **Volume deformation correction** (ViewPoint) for stretched faces at edges of wide-angle frames.
- **Compressed Linear DNG export**.

**Weaknesses vs LrC:** Weak DAM (no face recognition, no publish services, no map/book/slideshow/print layout depth, limited keywording); slower processing and export when DeepPRIME XD is on; no generative tools, no sky replacement; no tethering; no mobile/cloud; ecosystem split across several paid apps; Sony compressed RAW gaps reported at PL10 launch; Mac folder-management limitations.

Sources: [PetaPixel PhotoLab 10](https://petapixel.com/2026/09/01/dxo-releases-photolab-10-with-ai-add-ons-masking-and-color-grading-updates/), [DPReview PhotoLab 10](https://www.dpreview.com/news/dxo-photolab-10-announcement/), [Camera Jabber pricing](https://camerajabber.com/photography-news/dxo-photolab-10-price-specifications-and-availability-announced/), [Luminous Landscape 9.6](https://luminous-landscape.com/dxo-photolab-9-6-deepprime-xd3-support-for-every-camera-and-storage-solutions/), [Photography Life PL9 review](https://photographylife.com/reviews/dxo-photolab-9), [PhotoWorkout PureRAW 6](https://www.photoworkout.com/dxo-pureraw-6-cp-plus-2026/), [Photography Life PureRAW 6](https://photographylife.com/dxo-pureraw-6-review-amazing-compression)

---

## 5. ON1 Photo RAW 2026 (2027 coming this fall)

**Version:** Photo RAW **2026** (2026.3 latest update) with **2027** announced "this fall"; buying 2026 before Sept 30 includes free 2027. Editions: Photo RAW (perpetual, 2 computers), Photo RAW MAX (perpetual, 3 computers, adds plugins, Restore AI, Generative Erase, Culling Assistant, cloud AI rendering), Photo RAW MAX subscription (200 GB ON1 Cloud Sync, mobile), ON1 Photo Studio subscription (1 TB, 5 installs). Prices are shown only at checkout. macOS, Windows, iOS, Android.

**Feature inventory**
- RAW: own engine ("stronger RAW engine" in 2027), NoNoise AI (2027: specialized models for wildlife, astro, portrait, macro), **Tack Sharp AI** deblur, Resize AI 2026 (integrated super-resolution for print enlargements)
- AI: Brilliance AI, Super Select AI (tap to select object), AI subject/background masks, Portrait AI (skin, eyes, teeth), Sky Swap AI, Perfect Eraser, Generative Erase (MAX), **Keyword AI**, Culling Assistant (2027 MAX: similar groups, closed eyes, OOF)
- Creative: Effects module with filter stacking, **Depth Lighting**, **Split Field** (split-diopter), **Double Exposure**, **Motion Filter** (ICM simulation), film looks
- Layers, compositing, masking, HDR, **Focus Stacking**, Panorama
- **Negative Mode** (film negative inversion), Grayscale editing, separate H/V keystone with auto-scale
- DAM: **Browse module works without import** (folders, albums, cloud), ratings, keywords, Folder Actions (2027: automate metadata, presets, exports)
- Tethering (Canon, Nikon), printing, Flickr share, plugin to Photoshop/LrC (MAX)

**What it does that LrC doesn't (or does worse)**
- **Layers + compositing** and a full effects stack in one app.
- **Focus stacking.**
- **Sky Swap AI.**
- **Keyword AI** writes keywords into metadata; LrC has none.
- **Tack Sharp AI** (motion/focus blur recovery) and genre-specific denoise models.
- **Resize AI** super-resolution with more control than LrC's fixed 2x Super Resolution.
- **Negative film mode.**
- **Browse without import** plus Folder Actions automation.
- **Creative optical effects** (split diopter, ICM motion, depth lighting).

**Weaknesses vs LrC:** Performance and polish lag; masking and RAW quality historically a step behind; DAM less mature at huge catalog scale; tethering only Canon/Nikon; opaque pricing tiers; mobile/cloud is subscription-only.

Sources: [ON1 what's new 2027](https://www.on1.com/products/photo-raw/whats-new/), [ON1 features](https://www.on1.com/products/photo-raw/features/), [ON1 buy](https://www.on1.com/products/photo-raw/buy/), [ON1 2026 launch](https://www.on1.com/blog/on1-photo-raw-2026-is-here-the-future-of-raw-photo-editing-with-ai/), [ON1 2026 press release](https://www.on1.com/press/on1-announces-photo-raw-2026-with-advanced-ai-tools-masking-layers-and-new-creative-filters/)

---

## 6. darktable (open source)

**Version:** **5.6.1** (Aug 2026; 5.6.0 released June 21, 2026). 5.4 in Dec 2025. **5.8 in development**. Free, GPL; Linux, Windows, macOS.

**Feature inventory**
- **Scene-referred pipeline** (linear, 32-bit float), choice of display transforms: filmic rgb, sigmoid, **AgX** (5.4); color calibration (CAT, channel mixer), color balance rgb, tone equalizer, diffuse or sharpen, exposure/highlight reconstruction (guided laplacians), rgb curve/levels
- Demosaic: multiple algorithms (RCD, AMaZE, LMMSE, VNG, Markesteijn for X-Trans, dual demosaic); **Capture Sharpening** in demosaic (5.4)
- **5.6 AI subsystem (optional, local ONNX):** **AI object mask** (SAM 2.1 / SegNext click-to-segment with DenseCRF refinement), **neural restore** module (raw denoise, denoise, upscale, batch/tiled), Lua API for AI scripting
- **colorharmonizer** (rotate hues toward complementary/triadic/etc. harmonies in UCS), HEIF export (8/10/12-bit)
- Masks: drawn (path, ellipse, gradient, brush) + parametric (luminance, chroma, hue per channel) combined with boolean logic, mask refinement, **per-module blend modes and masks on every module**, multiple module instances
- DAM: lighttable with ratings, color labels, hierarchical tags, metadata, collections, filtering, map view, geotagging, **culling layout**, multiple workspaces (5.4)
- Tethering (via gphoto2), printing (Linux/macOS; Windows printing coming in 5.8), slideshows, styles, **Lua scripting**
- **5.8 (dev):** Spektrafilm (physics-based film simulation incl. grain, halation, diffusion), HDR bracket auto-alignment, **headless darktable-mcp server so AI agents can drive the RAW pipeline over MCP**, saturation curve module, contrast & texture module, GTK4 prep

**What it does that LrC doesn't (or does worse)**
- **Every module is maskable** with parametric + drawn mask combos and blend modes; multiple instances of any module. LrC masks only apply a fixed subset of sliders.
- **Choice of tone mapper / scene-referred workflow** (filmic, sigmoid, AgX) and a physically-motivated color calibration (CAT) module.
- **Choice of demosaic algorithm** and capture sharpening in demosaic.
- **Local AI (SAM2) segmentation, denoise, upscale fully offline**, no cloud or credits.
- **Color harmony tool.**
- **Scriptable (Lua) and, soon, agent-drivable via MCP.**
- Free and open, no lock-in; XMP sidecars.

**Weaknesses vs LrC:** Steep learning curve, UI complexity; slower on big batches without OpenCL; no generative fill; weaker lens-profile coverage (lensfun); AI features optional/off by default and need model downloads; no mobile/cloud sync; no face recognition; fewer third-party presets/plugins.

Sources: [darktable 5.6.0](https://www.darktable.org/2026/06/darktable-5.6.0-released/), [darktable 5.4.0](https://www.darktable.org/2025/12/darktable-5.4.0-released/), [darktable 5.6.1](https://www.darktable.org/2026/08/darktable-5.6.1-released/), [RELEASE_NOTES (5.8 dev)](https://github.com/darktable-org/darktable/blob/master/RELEASE_NOTES.md), [9to5Linux 5.6](https://9to5linux.com/darktable-5-6-open-source-raw-image-editor-released-with-new-ai-features)

---

## 7. RawTherapee and ART

**RawTherapee 5.13** (July 26, 2026). Free, GPL; Linux, Windows, macOS.
- Non-catalog file browser; per-image PP3 sidecars
- Demosaic choice (AMaZE, RCD, DCB, LMMSE, dual-demosaic, pixel-shift combining), **Capture Sharpening** (deconvolution) at RAW stage, now also in Selective Editing (5.13)
- **Selective Editing (Local Adjustments)**: spot-based local edits with many tools (color & light, shadows/highlights, wavelets, retinex, denoise, tone mapping, log encoding, CIECAM)
- Color Appearance (CIECAM02/16), Gamut Compression, Abstract Profile, GHS (Generalized Hyperbolic Stretch), new Michaelis-Menten tone mapper (5.13), default working profile changed to **Rec2020** (5.13)
- Pixel Shift, dual illuminant DCP, wavelet levels, Retinex, dynamic range compression

**ART 1.26.9** (Sept 16, 2026), fork of RawTherapee by Alberto Griggio:
- Simpler UI; drawn + parametric local-editing masks
- **ACES CLF LUTs via OpenColorIO and CTL scripts**, plugin system for custom input/output formats incl. **true HDR output**
- Easy integration of external tools including **AI masking and denoising**
- Better XMP handling (ratings, labels), permanent snapshots, auto perspective, inspector mode

**What they do that LrC doesn't:** choice of demosaic and deconvolution capture sharpening; pixel-shift combining; CIECAM color appearance modeling; wavelet-based local contrast/denoise; ACES/OCIO/CTL color pipelines (ART); true HDR output formats (ART); free.

**Weaknesses vs LrC:** No DAM to speak of (browser only, no catalog/keywords at scale), no AI masks natively (ART via external tools), no mobile/cloud, no tethering, no printing, dense UI, slow previews.

Sources: [RawTherapee 5.13](https://rawtherapee.com/downloads/5.13/), [ART home](https://artraweditor.github.io/), [ART releases](https://github.com/artraweditor/ART/releases)

---

## 8. ACDSee Photo Studio Ultimate 2026

**Version:** Ultimate 2026 (Sept 2025; 2027 edition expected around now, not verified). **Windows only** (Mac gets the lesser ACDSee Photo Studio for Mac). Perpetual ~$149.99 (1 year of updates) or ACDSee 365 Home $89/yr / $8.90/mo (up to 5 devices, cloud storage).

**Features:** Manage mode (browse without import, catalog DB, keywords, categories, ratings, **People mode** face detection/recognition), Develop mode (RAW, local adjustments, AI masks), **Edit mode with pixel layers, adjustment layers, blend modes**, AI Denoise, **AI Hair Masking**, AI Presets (adjustments bound to AI masks), Face Edit (with Splotch removal), AI Sky replacement, AI Remove **(some unverified)**, JPEG XL, video metadata, multi-threaded Activity Manager, privacy-focused on-device AI.

**What it does that LrC doesn't:** combined Lightroom + Photoshop-style **pixel layer editor** in one app; **browse without import** with a very fast viewer; face-shape editing (Face Edit); local/on-device AI marketed as privacy-first; perpetual license.

**Weaknesses vs LrC:** Windows-only; five modes feel fragmented; layered edits require proprietary .acdc; RAW rendering and AI masks generally behind Adobe; small plugin/preset ecosystem; no mobile editing parity.

Sources: [Digital Camera World review](https://www.digitalcameraworld.com/tech/software/acdsee-photo-studio-ultimate-2026-review), [PetaPixel ACDSee 2026](https://petapixel.com/2025/09/17/acdsee-photo-studio-ultimate-2026-introduces-privacy-focused-ai-powered-editing/)

---

## 9. AI culling / editing services: Aftershoot, Imagen, Narrative

### Aftershoot
- **Model:** subscription. AI Culling from $10/mo, AI Editing from $30/mo, AI Retouching from $20/mo; full bundle (unlimited cull + edit + retouch) from **$45/mo billed annually**. macOS, Windows. **Local/offline processing**, files stay on machine.
- **Features:** culling reads 30+ factors, groups true duplicates vs intentional variations, AI Automated and AI Assisted modes, learns from your picks; **AI Profiles** trained on 2,000-5,000 of your edited images; import LrC presets; built-in **RAW editor**; **AI Retouching** (skin, hair, glasses glare, backgrounds, distraction removal, backdrop swap, clothing refinement) as adaptive presets; **client galleries** with face search, proofing, print ordering; writes XMP for LrC.
- **Beyond LrC:** personal-style AI editing; batch AI retouching presets; gallery delivery integrated; culling that learns your taste.

### Imagen AI
- **Model:** per-photo, from $0.05/photo pay-as-you-go ($7/mo minimum); annual tiers 18K-72K photos (36K ≈ $127.50/mo). Cloud processing.
- **Features:** Personal AI Profile learned from your LrC or Capture One catalog; Culling Studio; AI crop/straighten, subject mask, smooth skin, retouching; returns edits as LrC/C1 settings.
- **Beyond LrC:** edits ship back as native LrC develop settings so they stay editable; learns style from catalog history; ~0.5 s/photo claimed.

### Narrative Select
- **Model:** Lite $10/mo, Standard $20, Premium $40 (2 users), Ultra $60 (4 users) annual (monthly $15/$29/$59/$79). Unlimited images.
- **Features:** fast RAW import (~3 s claimed), per-face eye and focus assessment, **Close-Ups panel** showing every face in a frame, Scenes grouping, first-pass indicators, Survey mode, Ship to Lightroom / Photoshop / Capture One, AI presets and personal AI presets trained on your edits, AI straightening.
- **Beyond LrC:** the **Close-Ups face panel per frame** and "a better frame likely exists" hints; multi-seat plans for teams.

**Shared weakness vs LrC:** not full editors/DAMs (Aftershoot is closest); rely on LrC or C1 for finishing; subscriptions; Imagen is cloud and per-image.

Sources: [Aftershoot](https://aftershoot.com/), [Aftershoot pricing blog](https://aftershoot.com/blog/aftershoot-pricing/), [Imagen pricing (FilterPixel summary)](https://filterpixel.com/imagen-ai-pricing), [PetaPixel Imagen promo](https://petapixel.com/2026/05/26/imagen-is-offering-full-ai-editing-access-for-10-just-in-time-for-peak-season/), [Narrative pricing](https://narrative.so/pricing)

---

## 10. Radiant Photo 2

- **Model:** perpetual $159 (app + plugin), upgrade $99; workflow packs $79 each or $349 all; optional Radiant Toolkit $50/yr. macOS, Windows. Standalone and plugin for Photoshop, **Lightroom Classic**, PaintShop Pro.
- **Features:** AI scene detection auto-selects a genre-specific "Smart Adjustment"; **Pixel Fidelity** per-pixel correction; Face Light; 10-point skin-tone detection and color-cast correction (Portraiture workflow); 16-bit; Smart Batch Export; explicitly non-generative.
- **Beyond LrC:** one-click, scene-aware auto-correction that is consistently better than LrC Auto for JPEG/TIFF volume work; skin-tone-aware color correction.
- **Weaknesses:** not a RAW developer or DAM; works on rendered files; limited manual control.

Sources: [Radiant Photo 2](https://radiantimaginglabs.com/radiant-photo-2/), [Fstoppers launch](https://fstoppers.com/news/radiant-photo-2-arrives-promising-next-generation-photo-editing-686083), [DCW review](https://www.digitalcameraworld.com/cameras/radiant-photo-2-review)

---

## 11. Apple Photos (macOS 27 / iOS 27) and Google Photos

### Apple Photos (macOS 27 "Golden Gate", iOS 27; fall 2026)
- New Apple Intelligence tools: **Clean Up** upgraded (larger objects, fast vs high-quality modes), **Extend** (generative outpaint for aspect ratio/straightening), **Spatial Reframe** (3D scene reconstruction to change camera position after capture)
- Library: **star ratings (1-5)** finally added (off by default), "Captured by Me", full-res iCloud Shared Albums (accessible to Android/Windows via iCloud.com), prioritized sync
- Free with device, on-device AI, ProRAW support
- **Beyond LrC:** Spatial Reframe (perspective change via 3D); zero-setup cross-device sync; on-device privacy.
- **Weakness:** shallow RAW controls, no masks, no keywords hierarchy at pro scale, no tethering.

Sources: [9to5Mac macOS 27 Photos](https://9to5mac.com/2026/09/17/macos-27-whats-new-for-the-photos-app/), [AppleInsider iOS 27 Photos](https://appleinsider.com/articles/26/06/09/apple-intelligence-gives-photos-in-ios-27-its-biggest-editing-upgrade-in-years), [MacRumors](https://www.macrumors.com/2026/04/28/ios-27-apple-intelligence-photo-editing-tools/)

### Google Photos
- **Help me edit** conversational editing (text or voice) powered by Nano Banana: "remove my sunglasses", "open eyes", "make X smile"; personalized edits that use your face groups to fix a person using their other photos; Create with AI templates; **Ask** button and Ask Photos natural-language search in 100+ countries; Magic Editor, Magic Eraser, Best Take, Photo Unblur.
- **Beyond LrC:** natural-language editing and search; identity-aware fixes (open eyes/expressions using other photos of that person).
- **Weakness:** not a RAW workflow; generative edits alter reality; cloud-only.

Sources: [TechCrunch Google Photos AI](https://techcrunch.com/2025/11/11/google-photos-adds-new-ai-features-for-editing-expands-ai-powered-search-to-over-100-countries/), [Android Police Nano Banana](https://www.androidpolice.com/6-new-ai-features-for-google-photos/)

---

## 12. Photo Mechanic (Camera Bits)

- **Products:** Photo Mechanic (current, native Apple silicon) $14.99/mo, $149/yr or **$299 perpetual**; Photo Mechanic Plus (catalog) $24.99/mo, $249/yr or $399 perpetual. Up to 2 computers. macOS, Windows. Legacy PM 6 is Intel-only on Mac and will stop running after macOS 27 (Rosetta end); **PM Plus sales discontinued** with no Apple-silicon Plus version.
- **Features:** ultra-fast ingest from multiple cards with renaming and backup; contact sheets using **embedded JPEG previews** (instant browsing of RAW); IPTC/XMP templates, **code replacements** (shortcut codes expand to names/captions, e.g. player numbers), variables, batch captioning; FTP/upload for wire services; Plus adds multi-drive catalog.
- **Beyond LrC:** far faster first-look culling on fresh cards (no preview rendering); code replacement captioning; newsroom FTP/transmission; LrC import + standard previews is much slower.
- **Weaknesses:** no editing, no AI culling (no eye/blur detection or duplicate grouping as of 2026 updates); product-line confusion over Plus.

Sources: [Camera Bits buy page](https://home.camerabits.com/get-photomechanic/), [Camera Bits PM6 on Mac](https://home.camerabits.com/photo-mechanic-6-on-mac-its-time-to-plan-ahead/), [FilterPixel comparison](https://filterpixel.com/blog/photo-mechanic-ai-alternative)

---

## 13. Zoner Studio (formerly Zoner Photo Studio X)

- **Model:** $59/yr ($5.99/mo), Family $98/yr (20 GB cloud, galleries). **Windows only.** EISA award 2025-2026.
- **Features:** Manager (browse without import, catalog), Develop (RAW, AI Subject/Background/Sky/Object masks since spring 2025), Editor (pixel layers), Video editor with 500+ music tracks, photo books/calendars; **Summer 2026:** **Photo Stacking tools** (focus stacking, exposure bracketing/HDR, panoramas, long-exposure simulation, **moving-object removal**, animations), **Autostack** grouping of series, new Search with rich metadata filters, **Smart Healing**. Fall 2026: photo-book rulers/grids, video subtitles.
- **Beyond LrC:** **focus stacking, long-exposure simulation from a burst and moving-object removal**; built-in video editor; layers; cheap.
- **Weaknesses:** Windows-only; RAW quality and AI masks behind Adobe; small ecosystem.

Sources: [Zoner Summer 2026](https://www.zoner.com/en/summer-2026-update), [SLR Lounge](https://www.slrlounge.com/zoner-studios-2026-summer-update-brings-focus-stacking-panorama-stitching-and-smarter-raw-library-workflows/), [Capterra pricing](https://www.capterra.com/p/191817/Zoner-Studio/pricing/)

---

## 14. Exposure X7 (Exposure Software)

- **Status:** Exposure X7 remains current (no X8); development appears slowed (reports that the lead programmer took other employment). ~$129 perpetual; bundle with Snap Art and Blow Up. macOS, Windows; standalone or Photoshop/LrC plugin.
- **Features:** catalog-free browsing, non-destructive **layers** with masks and blend modes, 500+ presets with strong **film emulation** (grain, bokeh, light leaks, overlays), AI masking, portrait retouch, LUTs, lens correction, denoise.
- **Beyond LrC:** film simulation depth and grain realism; layers; no import step.
- **Weaknesses:** stagnating development; weaker AI; small user base.

Sources: [Exposure Software](https://exposure.software/), [DPReview X7 review](https://www.dpreview.com/reviews/exposure-x7-software-review-more-powerful-masking-and-a-ui-that-adapts-to-your-needs)

---

## 15. Takeaways for a Lightroom-class product spec

1. **Parity is moving fast.** Adobe has closed AI culling, dust removal, feathered AI masks and generative expand in 2025-26. Differentiation now sits in: pipeline quality (DxO optics + DeepPRIME XD3), studio/tether collaboration (Capture One), personalization (Aftershoot/Imagen), and openness (darktable MCP/Lua).
2. **Must-consider differentiators nobody in Adobe offers:** multi-user real-time session editing; wireless tethering with live AI cull; lens-softness correction from measured optics; local (brushable) AI denoise; depth-band masks; per-person facial-part segmentation; editing style learned from the user's own catalog; AI keyword writing; focus stacking and burst-based long-exposure/object removal; every-adjustment-maskable pipeline with layers; negative film conversion; browse-without-import; perpetual licensing and a scriptable agent interface.
3. **Common competitor weaknesses to avoid:** thin DAM (keywords, faces, publish, print/book), slow large-catalog performance, fragmented app line-ups (DxO), confusing expiring-AI licensing (Luminar), and features gated to top tiers (Capture One Studio).
