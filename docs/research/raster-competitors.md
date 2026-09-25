# Photoshop Competitors: Raster / Pixel Editors (as of 2026-09-24)

Scope: Affinity (Canva), Pixelmator Pro / Photomator (Apple), GIMP 3.x, Krita, Photopea, Krea / Canva AI, Topaz (Photo / Gigapixel), Evoto and Retouch4me.
Method: facts were checked against official pages, release notes, and reviews (DCW, CG Channel, Amateur Photographer, 9to5Mac, Apple Newsroom, Topaz docs). Items marked *(unverified)* come from background knowledge and were not reconfirmed this session.

---

## Executive summary: where competitors beat Photoshop

| Theme | Who does it better than Photoshop | Why it matters for a spec |
|---|---|---|
| **Price / licensing** | Affinity (free core), GIMP/Krita (free, FOSS), Photopea (free, ads), Pixelmator Pro ($49.99 one-time) | Photoshop is subscription-only. "Free pro pixel editor" is now a normal expectation. |
| **Non-destructive filters without Smart Objects** | Affinity Live Filter layers; GIMP 3 GEGL NDE filters; Krita filter layers/masks and *transform masks* | Photoshop Smart Filters need a Smart Object conversion, and you can't paint or clone on the result. |
| **HDR / 32-bit / OCIO** | Affinity (32-bit, OCIO 2.5, Tone Map persona); Krita (float HDR painting, Wayland HDR on Linux, OCIO) | Photoshop's 32-bit mode loses many tools, and it has no OCIO pipeline. |
| **Astro stacking** | Affinity (Astrophotography stacking plus new Astrophotography Studio in 3.3) | Photoshop has no astro calibration or stacking workflow. |
| **One app for vector, pixel and layout** | Affinity Studios (Pixel/Vector/Layout in one document); now INDD import | Photoshop users have to round-trip to Illustrator and InDesign. |
| **Built-in scripting studio** | Affinity 3.3 Scripting Studio (JavaScript across all studios) | Photoshop has UXP/ExtendScript but no in-app IDE. |
| **Browser, zero install, PSD-native** | Photopea (runs locally in the browser, opens PSD/XCF/Sketch/XD/Figma *(unverified list)*) | Photoshop on the web is limited and needs sign-in. |
| **Dedicated AI restoration quality** | Topaz Photo (Wonder 3.5, Recover Faces v3, Super Focus v3, Dust & Scratch v2); Gigapixel (up to 16x, Redefine/Recover generative upscalers) | Photoshop's Super Resolution/Generative Upscale and Denoise lag behind for heavy restoration. |
| **Automated high-end portrait retouch at batch scale** | Evoto (per-face batch sync, culling, tethering, video retouch); Retouch4me (texture-preserving Heal, Dodge & Burn, perpetual local licenses) | Photoshop has no identity-aware batch retouch, and neural filters are weak for pro retouch. |
| **Apple-platform integration** | Pixelmator Pro 4 (iPad and Mac parity, iCloud, Final Cut / Keynote / Pages round-trip, Image Playground) | Photoshop iPad lacks feature parity. |
| **Real-time generative canvas** | Krea (real-time canvas, region edit, relight, 60+ models, voice mode) | Photoshop Firefly/partner models are not real-time. |

---

## 1. Affinity (by Canva) — the unified app replacing Affinity Photo / Designer / Publisher

**Version / release:** Affinity 3.0 launched on 30 Oct 2025 as one app combining Photo, Designer and Publisher. The current release is **Affinity 3.3 (September 2026)**, which adds 60+ features.
**Price:** **Free** with a free Canva account. There are no watermarks or feature limits in the Pixel, Vector and Layout studios. **Canva AI features need Canva Pro (~$144/yr) or Business (~$250/yr).** Legacy V2 perpetual licenses still work.
**Platforms:** macOS and Windows. iPad was promised for 2026 but had not shipped as of the 3.3 coverage. No Linux.

### Feature inventory (Pixel studio, formerly Affinity Photo)
- **Layers / masks:** pixel, image, vector and adjustment layers, plus live filter layers; layer and vector masks; clipping; blend ranges; linked/embedded documents.
- **Adjustments:** full set of non-destructive adjustment layers (curves, levels, HSL, white balance, split toning, selective color, LUT, etc.). New in 3.3: **Saturation/Hue Curves adjustment** (spline-based) and **Channel Inversion**.
- **Live filters:** most filters (blurs incl. depth-of-field, lighting, sharpening incl. clarity, distortions, noise) can be applied as **non-destructive Live Filter layers**. They stay editable and need no Smart Object step. 3.3 adds **Multi Band and High Pass edge sharpening**.
- **Selection:** Selection Brush, Refine (edge/matte), on-device ML **Object Selection**, channel-based selections.
- **Retouch:** Inpainting brush/fill, healing, patch, clone, frequency separation filter, Liquify Persona (destructive), and 3.3's **Deform Tool** (bone-chain/pin warping that can re-pose a subject) plus the **Mask Tool** (brush or gradient automatically creates and paints a mask).
- **RAW:** Develop studio with lens corrections, tone/color, basic gradient and brush masks. **Global RAW develop presets** arrived in 3.3. No AI denoise in the free tier.
- **HDR / panorama / focus stacking / astro:** built-in **HDR Merge with a Tone Map persona** (reviewers call it among the best HDR tools available), panorama stitching, focus merge, **astrophotography stacking**, and a new **Astrophotography Studio workspace** in 3.3.
- **Color / bit depth:** 8/16/32-bit per channel, RGB/CMYK/LAB/Grey. **OCIO 2.5** in 3.3. ICC soft-proofing.
- **Text / vector:** full Vector studio (pen, shape builder, image trace, mesh gradients, new diffusion gradient, Blend Tool, G2 curvature visualization) and Layout studio (master pages, preflight, **InDesign INDD import** in 3.3, ePub). All inside one document.
- **Automation:** macros recorder and **Batch Process** that applies macros without opening files. **Scripting Studio (JavaScript) across all studios, new in 3.3.**
- **Plugins:** Photoshop-compatible 8bf plugin support *(unverified for 3.x; it existed in v2)*. Topaz Photo and Retouch4me both list Affinity Photo as a supported host.
- **AI (Canva paid):** local models (noise reduction, motion-blur reduction, mixed-light correction, SDR-to-HDR, object/region detection). Cloud models (AI upscaling, image decomposition into layers, **Layer to 3D**, Generative Fill/Expand, Generate Image/Vector).
- **Formats:** native .af, plus PSD import/export (partial), PDF, SVG, EPS, TIFF, EXR, HDR, JPEG, PNG, WebP, RAW, INDD (import), ePub.

### Where Affinity beats Photoshop
1. **Free, full-featured pro pixel editor.** Only the generative AI costs money.
2. **Live Filter layers.** Non-destructive filters that don't need Smart Objects and can be reordered and masked as ordinary layers.
3. **One app for pixel, vector and layout.** Switch studios in the same document. Photoshop needs Illustrator and InDesign for this, and Affinity now imports INDD.
4. **Tone Map persona and 32-bit with OCIO 2.5.** A better HDR/VFX color pipeline than Photoshop's limited 32-bit mode.
5. **Astrophotography stacking and a dedicated Astro Studio.** Photoshop has none.
6. **Built-in Scripting Studio (JavaScript IDE)** plus a macro recorder with headless batch.
7. **Deform Tool with bone chains.** Re-poses a subject, which goes beyond Puppet Warp.
8. **Custom studios.** You can combine tools and panels from any studio into your own workspace.
9. **Layer to 3D and image decomposition** (Canva AI) have no direct Photoshop equivalent.

### Weaknesses vs Photoshop
- Generative AI is behind a Canva subscription, and model quality is not comparable to Firefly/partner models in reviews.
- The RAW Develop studio lacks AI denoise (free tier), AI masking, and Adobe-level perspective/upright correction. There is no cataloguing.
- Liquify is still destructive, and PSD round-trip fidelity is imperfect (Smart Objects, some layer styles).
- No iPad version yet (v2 iPad apps are frozen). No Linux.
- Some users distrust the Canva account requirement and data policies. Plugin ecosystem is smaller.

**Sources:** https://www.cgchannel.com/2026/09/canva-releases-affinity-3-3/ · https://en.wikipedia.org/wiki/Affinity_(software) · https://www.digitalcameraworld.com/tech/software/affinity-review · https://amateurphotographer.com/review/affinity-by-canva-review/ · https://www.canva.com/newsroom/news/all-new-affinity/ · https://www.macrumors.com/2025/10/31/canva-relaunches-affinity-free-app/

---

## 2. Pixelmator Pro (Apple) and Photomator

**Version:** **Pixelmator Pro 4.0** was released 28 Jan 2026 alongside the launch of **Apple Creator Studio**. Apple completed the acquisition in Feb 2025. Updates followed in April and June 2026 (Creator Studio integration, Content Hub, advanced image generation, shape generation).
**Price:** **$49.99 one-time** (Mac App Store), or **Apple Creator Studio at $12.99/mo or $129/yr** (bundle includes Final Cut Pro, Logic Pro, Pixelmator Pro, and more; education $2.99/mo or $29.99/yr).
- **Subscription-only features:** the **Warp tool**, the Liquid Glass UI, the iPad app, advanced image generation, and Content Hub are Creator Studio exclusives. The one-time version is falling behind.
**Platforms:** macOS 26+ (Apple Silicon) for the Creator Studio build, macOS 12+ for the one-time build. **iPadOS 26 (Pixelmator Pro for iPad, new Jan 2026)**. Apple-only.

### Feature inventory
- Layers, non-destructive adjustments and effects, masks, clipping, layer styles, shape and vector tools, text on path, and video layers with grading (no keyframes).
- **ML tools:** ML Super Resolution (3x upscale), ML Denoise, ML Enhance, ML Deband, ML Crop, Select Subject, Remove Background, ML Match Colors, Repair (ML inpainting), and AI selective clarity.
- **Warp (new in 4.0):** 12 smart warp presets (cylinder, arc, flag…), 3x3/4x4/5x5 warp grids, and warp-powered product and apparel mockups.
- **Generative:** Image Playground / Apple Intelligence integration (with ChatGPT extension), plus advanced image generation from natural-language prompts (June 2026).
- **RAW:** Apple RAW engine with 750+ formats. New cameras are added regularly (e.g., Sony A7 V, GFX 100S II, GFX 100RF, April 2026).
- **Color:** LUTs, Display P3 and wide-gamut, 16-bit, HDR image editing and export *(HDR export unverified for 4.0)*.
- **Automation:** AppleScript and Shortcuts actions *(unverified in this session; long-standing Pixelmator Pro feature)*.
- **Ecosystem:** open any image from **Keynote / Pages / Numbers** for editing and auto-save it back. **Send a frame from Final Cut Pro** to Pixelmator Pro. iCloud sync between iPad and Mac.
- **Formats:** PSD (layered import/export, some effects lost), PXD, JPEG, PNG, HEIF, TIFF, WebP, SVG, PDF, GIF, MP4, ProRAW.

### Photomator
Photomator is a separate photo-only, Lightroom-style editor for Mac, iPad, iPhone and Vision Pro. It works directly on the Apple Photos library (no import) and offers non-destructive color adjustments, 750+ RAW formats, Auto Enhance, Repair, Super Resolution, Denoise, ML Crop, Select Subject/Sky/Background, batch editing, and LUTs. It is a Lightroom competitor more than a Photoshop competitor. Its placement inside Creator Studio is unclear: Apple's June 2026 newsroom post only mentions Pixelmator Pro.

### Where it beats Photoshop
1. **One-time purchase for $49.99** (while it lasts), or a $129/yr bundle that also includes Final Cut Pro and Logic.
2. **Deep OS and app integration:** round-trip editing from Keynote, Pages, Numbers and Final Cut Pro, Apple Photos extension, Shortcuts.
3. **Real iPad/Mac parity** with iCloud document sync. Photoshop iPad is still a reduced app.
4. **On-device Core ML tools** (Super Resolution, Denoise, Deband). Deband has no Photoshop equivalent.
5. **Smart warp presets and ready-made mockups** make packaging and apparel mockups fast.
6. **Photomator's in-place editing of the Photos library.**

### Weaknesses
- Apple-only. No CMYK print workflow worth mentioning *(unverified)*. No plugins (no 8bf/UXP). No actions-level macro recorder.
- No HDR merge, panorama, or focus stacking. Limited PSD fidelity.
- Two-tier product: the best new features are subscription-only. Auto layer selection is unreliable (DCW).
- Generative features depend on Apple Intelligence / ChatGPT and trail Firefly in control.

**Sources:** https://www.apple.com/newsroom/2026/06/apple-creator-studio-gets-smarter-faster-and-more-connected/ · https://www.digitalcameraworld.com/tech/software/pixelmator-pro-4-review · https://9to5mac.com/2026/01/28/pixelmator-pro-launches-on-ipad-heres-what-the-new-app-can-do/ · https://9to5mac.com/2026/04/11/pixelmator-pro-liquid-glass-exclusive-to-creator-studio-subscribers/ · https://9to5mac.com/2026/04/09/apple-updates-creator-studio-apps-including-logic-pro-and-pixelmator-pro-more/ · https://en.wikipedia.org/wiki/Pixelmator_Pro · https://photomator.org/

---

## 3. GIMP 3.x

**Version:** **GIMP 3.2** (released 14 Mar 2026; point releases may follow). GIMP 3.0 shipped in March 2025.
**Price:** free, open source (GPL). **Platforms:** Linux, Windows, macOS.

### Feature inventory
- **GIMP 3.0 base:** GTK3 UI, **non-destructive GEGL filters** on layers (editable, reorderable, toggleable), multi-layer selection, auto-expanding layer boundaries, improved color management (babl/GEGL, space-aware), CMYK soft-proofing, and a PSD import overhaul.
- **3.2 additions:**
  - **Link Layers:** external images placed and transformed non-destructively, similar to linked Smart Objects.
  - **Vector Layers:** the Path tool creates editable fill/stroke shapes.
  - **SVG export.**
  - MyPaint brush upgrade with 20 new brushes and zoom/rotation awareness; Overwrite paint mode.
  - Better on-canvas text editing.
  - **CMYK Total Ink Coverage** readout.
  - Expanded **PSD layer-style import**.
  - Formats: APNG import, **multi-layer OpenEXR** load, **JPEG 2000** import/export, DDS BC7 export.
  - A GEGL filter browser for plug-in developers.
- **Other:** high bit depth (up to 32-bit float), Python 3 / Script-Fu / C plug-in API, G'MIC plugin ecosystem, RAW via darktable/RawTherapee hand-off, heal/clone/perspective clone, warp transform, cage transform.

### Where it beats Photoshop
1. Free and open source, and the only one here with **first-class Linux** support. Extensible via Python 3 and GEGL.
2. **Non-destructive GEGL filters on any layer without Smart Objects.** Mirrors Affinity's Live Filters.
3. **Perspective Clone** and a **Cage Transform** tool *(long-standing)*. The G'MIC plugin library covers hundreds of filters Photoshop lacks.
4. Reads a very broad set of formats (XCF, DDS, JPEG 2000, multi-layer EXR, APNG, PDF, SVG).

### Weaknesses
- No adjustment layers in the Photoshop sense. NDE filters cover part of that gap.
- No built-in RAW development, HDR merge, panorama, focus stacking, or AI features (no content-aware fill beyond Resynthesizer/G'MIC add-ons).
- CMYK is proof-only, with no native CMYK working space. UI/UX consistency still lags.
- Destructive warp and liquify tools.

**Sources:** https://www.gimp.org/news/2026/03/14/gimp-3-2-released/ · https://www.gimp.org/release-notes/gimp-3.2.html · https://www.phoronix.com/news/GIMP-3.2-Released · https://9to5linux.com/gimp-3-2-open-source-image-editor-officially-released-heres-whats-new

---

## 4. Krita (photo-relevant features only)

**Version:** **Krita 5.3.4 / 6.0.4** (15 Sep 2026). 5.3.0 and 6.0.0 were released together on 24 Mar 2026 from the same source (6.x = Qt6 and better Wayland; 5.3 is still recommended for production).
**Price:** free (GPL). Paid store builds exist on Steam, Microsoft Store and others. **Platforms:** Windows, macOS, Linux, Android/ChromeOS tablets.

### Photo-relevant features
- **Filter layers, filter masks, and *transform masks*.** Transforms, including **Liquify, Cage, Mesh and Perspective**, can live as non-destructive masks *(transform-mask modes from background knowledge)*. Clone layers; file layers (linked).
- **HDR:** 16/32-bit float painting. **HDR display output** (Windows, and now **Linux Wayland color-management protocol** in 6.0 with 10-bit). All blend modes checked for HDR so values don't clip. OCIO/LUT docker for exposure and gamma viewing. Radiance .hdr support.
- **Color management:** ICC plus OCIO. **Soft-proofing rebuilt** with black point compensation.
- **Formats:** PSD (shapes, vector masks, guides, text props), **JPEG-XL (multi-layer, multi-page, animated, CICP HDR)**, EXR, HEIF/AVIF, KRA.
- **Selection:** new selection toolbar, colorize mask, magnetic/similar selection.
- **Scripting:** Python API (extended in 5.3). Real-time recorder docker.
- **No** RAW development, HDR merge, panorama, AI features, or content-aware fill.

### Where it beats Photoshop
1. **Non-destructive transform masks**, including liquify-style warps as masks. Photoshop's Liquify Smart Filter is clunkier and can't be painted into.
2. **HDR painting and display on Linux via Wayland**, plus full-float blend-mode correctness.
3. **Multi-layer and animated JPEG-XL export** with CICP HDR metadata. Photoshop's JXL support is minimal.
4. Free; the brush engine and painting UX are much stronger than Photoshop's.

### Weaknesses
- Not a photo editor at heart. No RAW, AI, content-aware or automation for photographers. Heal/clone tools are basic. Large-file photo performance is weaker.

**Sources:** https://krita.org/en/posts/2026/krita-5.3.0-released/ · https://krita.org/en/release-notes/krita-5-3-release-notes/ · https://krita.org/en/posts/2026/roadmap-2026/ · https://en.wikipedia.org/wiki/Krita

---

## 5. Photopea

**Version:** Wikipedia lists 5.6 (Sept 2024) as the last numbered stable. The product ships continuously and recent additions include Puppet Warp, Normal Map filter and Oil Paint filter.
**Price:** free with ads. **Premium ~$5/mo, $15 per 90 days, or ~$50/yr**: no ads, 5 GB PeaDrive, **3,000 AI credits per month**, team plans. Self-hosting / white-label reportedly **$500–$2,000/mo**.
**Platforms:** any modern browser (Chrome, Firefox, Safari, Edge), installable as a PWA.

### Feature inventory
Photoshop-clone UI: layers, masks, channels, **Smart Objects**, adjustment layers, layer styles, text, vector shapes, pen, healing/patch/clone, content-aware tools *(unverified)*, Liquify, Puppet Warp, actions, and a scripting API. AI: background removal (limited free, generous on Premium), generative fill/replace (Stable Diffusion-based, credits). Formats: PSD (read/write with high fidelity), XCF, Sketch, XD, Figma, CDR, PDF, SVG, RAW/DNG *(format list partly from background knowledge)*.

### Where it beats Photoshop
1. **Zero install, runs anywhere, fully client-side.** Files stay local unless the user saves to cloud, which is better for privacy than Photoshop web.
2. **Opens PSD and competitor formats (XCF, Sketch, XD, Figma, CDR)** in one tool. It is a universal viewer and converter.
3. **Embeddable / white-label** via API, so SaaS products can embed a Photoshop-class editor. Adobe offers no equivalent.
4. About $50/yr against Photoshop's ~$275+/yr.

### Weaknesses
Single-threaded JS performance on large files. Limited 16/32-bit and color management. No RAW develop depth, HDR merge or focus stacking. Ads in the free tier. One-person dev team. AI is basic compared with Firefly.

**Sources:** https://en.wikipedia.org/wiki/Photopea · https://www.saasworthy.com/product/photopea/pricing · https://costbench.com/software/design/photopea/

---

## 6. Krea and Canva AI photo editing (brief)

**Krea (krea.ai):** browser-based multi-model creative suite (Flux, Ideogram, Veo 3.1, Kling, Runway, Luma, and 60+ others). Freemium with paid tiers.
- **Real-time canvas:** move, draw or prompt and see generation update live. **Voice mode** added in early 2026.
- **Krea Edit:** region-select plus prompt edits, **relight**, colorize, and a **camera-lens simulator re-render**.
- **Enhancer / upscaler up to 22K**, object and text removal, and video (lipsync, animate stills). An agent/enterprise tier also exists.
- **Beats Photoshop on:** real-time generative iteration, choice of third-party models, relighting, and lens re-rendering.
- **Weak:** it isn't a layer-based pixel editor, gives no pixel-precise manual control, and has no print color management.

**Canva AI (Magic Studio):** Magic Grab (split a flat photo into movable elements), **Magic Layers** (turn a generated or flat image into editable layers), Magic Edit (brush plus prompt), Magic Eraser, Magic Expand, Background Remover, Dream Lab generation, Bulk Create. Magic tools need a paid plan.
- **Beats Photoshop on:** turning a flattened image into editable layers (Magic Grab / Magic Layers) and template-driven bulk variations. It is also now the AI backend for Affinity's paid tier.
- **Weak:** low precision, and 8-bit sRGB-oriented output.

**Sources:** https://www.krea.ai/blog/krea-edit · https://www.krea.ai/ · https://www.canva.com/features/ai-photo-editing/ · https://moda.app/blog/canva-magic-studio · https://www.canva.com/help/using-magic-grab/

---

## 7. Topaz Labs: Topaz Photo (formerly Photo AI) and Gigapixel

**Licensing change:** **perpetual licenses ended September 2025.** Everything is now subscription.

- **Topaz Photo:** $39/mo (lower with annual billing; Photo AI legacy plans ~$199/yr). **v1.7.0, released 27 Aug 2026.** Subscription-only since Nov 2025.
- **Topaz Gigapixel** (redesigned app released 16 Sep 2025): Personal $29/mo or $149/yr; Pro $499/yr (full commercial use).
- **Topaz Studio** (all apps: Photo, Gigapixel, Video, Bloom, Web, Mobile): $69/mo or $399/yr.
- All plans include **unlimited cloud rendering** plus local GPU rendering.

**Platforms:** Windows and macOS desktop, web app, iPhone app. **Plugins for Photoshop, Lightroom Classic, Capture One, Apple Photos and Affinity Photo.** Photoshop integration comes in two forms: a Filter plugin and an Automate plugin.

### Topaz Photo models (as of v1.7)
- **Wonder** v1 → v2 → v3 → **v3.5 (Aug 2026):** one-shot denoise, sharpen and upscale with no sliders. v3.5 improves text and low-res/compressed sources and reduces repetitive-pattern artifacts.
- **Recover Faces v3**, **Denoise Max**, **Super Focus v3** (recovers mis-focused shots), Sharpen (Noise-Aware and Portrait variants), **Dust & Scratch v2**, Remove Tool v2, Adjust Lighting v3, Grain, Standard Max and High Fidelity v3 upscalers.
- **NeuroStream (2026):** Topaz says it cuts VRAM needs by up to 95%, so heavy generative models run locally.
- Batch processing and Autopilot auto-detection.

### Gigapixel models
Wonder 2 and 3 (generative), Standard, Standard Max, High Fidelity, Low Resolution, Text & Shapes, Art & CG, **Recover**, **Redefine** (diffusion upscaler with text prompt and creativity slider), and Face Recovery Gen 2. **Up to 16x.**

### Where it beats Photoshop
1. **Much stronger restoration and upscaling:** 16x with purpose-built models against Photoshop's 2x/4x Super Resolution and Generative Upscale.
2. **Super Focus**, which rescues mis-focused or motion-blurred shots. Photoshop's Shake Reduction was removed and has no real equivalent.
3. **Recover Faces** with identity-preserving face reconstruction for low-res or old photos.
4. **Dust & Scratch** automatic removal for scans.
5. **Local, private processing**, with the option of unlimited cloud rendering.

### Weaknesses
Not an editor (no layers or compositing). Now subscription-only, which caused user backlash. Generative models can hallucinate texture or text. Heavy GPU load.

**Sources:** https://www.topazlabs.com/ · https://www.topazlabs.com/topaz-gigapixel · https://docs.topazlabs.com/topaz-photo/release-summary · https://community.topazlabs.com/t/topaz-photo-v1-7-0-wonder-3-5/104557 · https://photorumors.com/2026/03/04/major-upgrades-to-topaz-photo-and-astra-now-live/ · https://www.cgchannel.com/2025/02/topaz-labs-releases-gigapixel-8/ · https://docs.topazlabs.com/topaz-photo/plugins/photoshop-and-photoshop-elements

---

## 8. Portrait retouching: Evoto AI and Retouch4me

### Evoto AI
**Version:** **Evoto Desktop 8.0** (2026 launch event), plus the Evoto App (mobile), Evoto Video, and Evoto Instant (tethering).
**Price:** credits, about **1 credit per exported image**; free basic edits; subscriptions **~$80–$1,205/yr (800–24,000 credits, 2–6 devices)**; from ~$9.99/mo; pay-as-you-go.
**Platforms:** Windows, macOS, iPad, iOS, web.

**Features:**
- **Per-face AI retouching:** skin with texture preserved, blemishes, face and body shaping, eyes, teeth, stray hair.
- Glasses-glare removal, clothing wrinkles, background cleanup and replacement, sky replacement, image expansion.
- Color grading and color matching; RAW support.
- **Batch sync using face recognition**, so each person gets their own settings across a shoot.
- **AI culling**, including 2026 "Story-Based Culling".
- Saved **Workflows**, presets, and an Asset Hub.
- **Tethered shooting with live AI retouch** (Evoto Instant).
- **Video portrait retouching**; gallery delivery and proofing.

**Beats Photoshop on:**
1. Identity-aware batch retouching across thousands of images.
2. Culling, tethering, retouching and delivery in one tool.
3. Live retouch while tethered.
4. Glasses glare and wrinkle removal as single sliders.
5. Consistent retouch across video.

**Weak:** per-image credit costs add up at volume. Cloud dependence and privacy questions. It is not a general compositor, and fine control is limited compared with manual frequency separation or dodge and burn.

**Sources:** https://www.evoto.ai/ · https://www.evoto.ai/launch26 · https://www.digitalcameraworld.com/tech/software/evoto-ai-review · https://www.saasworthy.com/product/evoto-ai/pricing

### Retouch4me
**Model:** individual AI plugins.
**Price:**
- **Perpetual local licenses** at about $124–$179 each (Heal, Dodge & Burn, Portrait Volumes, Face Make, Skin Tone, Skin Mask, Mattifier, Crop, Eyes, White Teeth, Stray Hairs, Color Match, Fabric, Dust, Clean Backdrop). Bundles are available. Frequency Separation and Color Match Free are free.
- **Cloud subscriptions:** Start $169/yr (2,400 retouches), Pro $299/yr, Business $759/yr. These include culling and AI color correction.
- **Video plugins** (OFX / Premiere / Final Cut Pro).

**Hosts:** Photoshop (panel), Lightroom, Capture One, Affinity Photo, Premiere, DaVinci Resolve, Final Cut Pro. Standalone app. Windows and macOS.

**Beats Photoshop on:** Photoshop has no automated **texture-preserving Heal** or **automatic Dodge & Burn** that look like human high-end retouching. Retouch4me outputs editable layers or masks, runs fully offline on a one-time license, and applies the same models to video.

**Weak:** buying plugins one by one gets expensive. It depends on a host app for layering. Results can over-process at default strength.

**Sources:** https://retouch4.me/pricing · https://retouch4.me/retouchplugins · https://fstoppers.com/reviews/retouch4me-just-got-whole-lot-better-699841 · https://www.aiarty.com/edit-photo/retouch4me-review.htm

---

## Product-spec takeaways (ranked by differentiation value)
1. **Non-destructive everything:** live filter layers, transform and liquify masks, link layers. No "convert to Smart Object" step (Affinity, GIMP 3, Krita).
2. **Free or perpetual core with paid AI credits** (Affinity plus Canva, Photopea, Pixelmator one-time, Retouch4me perpetual). This is now the market reference price.
3. **Built-in computational photography:** HDR merge with a strong tone mapper, panorama, focus stacking, astro stacking (Affinity).
4. **Specialist AI quality:** Topaz-class restoration (Wonder, Super Focus, Recover Faces, 16x) and Evoto/Retouch4me-class portrait automation with per-face batch.
5. **Modern pipelines:** OCIO 2.5, float HDR display, multi-layer JPEG-XL/EXR (Affinity, Krita).
6. **Cross-app unification:** pixel, vector and layout in one document (Affinity), OS-level round-trip (Pixelmator).
7. **Embeddable browser editor** (Photopea) and a **real-time generative canvas** (Krea).
8. **Scripting in the app** (Affinity Scripting Studio, Krita/GIMP Python).
