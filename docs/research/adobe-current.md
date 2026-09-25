# Adobe Photoshop & Lightroom: Current Feature Set (as of 2026-09-24)

Research note: helpx.adobe.com returned HTTP 403 to automated fetches, so version and feature data comes from Adobe News/Blog/Community announcements, Adobe's plan page, and secondary sources (Lightroom Queen, PetaPixel, DPReview, CG Channel, VideoHelp, Computer Darkroom, Fstoppers). Where sources disagree, both are noted.

---

## 0. Current versions (Sept 24, 2026)

| Product | Current release | Notes |
|---|---|---|
| Photoshop desktop | **27.10** (Aug 2026) | 27.11 is in public beta. Users report 27.10/27.11b open/save >50% slower on Win 11 |
| Photoshop LTS | 26.11.x (26.11.4 noted Mar 2026) | |
| Camera Raw | **18.6** (released ~Aug 25 to Sep 1, 2026) | Glow, Trim, Metadata panel |
| Lightroom Classic | **15.5.1** (Aug 31, 2026; 15.5 Aug 3, 2026) | No 15.6 yet. macOS 27 support expected in the next release |
| Lightroom (cloud) desktop | 9.5 / 9.5.1 | |
| Lightroom mobile | 11.5 | |
| Photoshop web / iPhone / iPad / Android | Continuously updated; no public version numbers verified | iPhone launched Feb 2025, Android beta June 2025. iPad and iPhone share one codebase |

Sources: https://www.cgchannel.com/2026/08/adobe-releases-photoshop-27-10/ · https://community.adobe.com/bug-reports-711/photoshop-27-10-and-beta-27-11-open-and-save-image-files-over-50-slower-than-the-previous-version-on-windows-11-1640337 · https://www.lightroomqueen.com/whatsnew/classic/ · https://www.lightroomqueen.com/whats-new-in-lightroom-2026-08/ · https://www.dpreview.com/news/adobe-adds-glow-to-camera-raw/ · https://www.computer-darkroom.com/blog/2026/08/25/camera-raw-late-august-2026/

---

## 1. Photoshop: chronology, mid-2024 to Sept 2026

Main version list source: https://www.videohelp.com/software/Adobe-Photoshop/version-history (cross-checked with the per-release sources below).

### 2024
- **25.9 (May 2024)**: **Adjustment Brush** released to GA (paints non-destructive Brightness/Contrast, Exposure, Vibrance, Hue/Sat and similar adjustments). Firefly Image 3 generative features were still in beta. https://www.cgchannel.com/2024/05/adobe-releases-photoshop-25-9/
- **25.11 (July 2024)**: **Generate Image** (text-to-image, Firefly Image 3) GA. Reference Image for Generative Fill. **Selection Brush tool**. Adjustment Brush improvements. Bullets & Numbering for type. Improved **Contextual Task Bar**. **Enhance Detail** for generative output. https://www.cgchannel.com/2024/07/adobe-rolls-out-generate-image-in-photoshop-25-11/ · https://petapixel.com/2024/07/23/photoshop-update-brings-generative-ai-and-adjustment-brushes-out-of-beta/
- **25.12 (Sept 2024)**: in-app notifications, minor changes.
- **26.0 (Oct 2024, "Photoshop 2025", MAX 2024)**:
  - **Distraction Removal** in the Remove tool: one-click removal of people, wires and cables, plus a "Distraction Finder" that highlights candidates.
  - Generative Fill, Expand, Generate Similar and Generate Background moved to the new Firefly Image Model 3. **Generate Background** and **Generate Similar** went GA.
  - **Generative Workspace (beta)**.
  - **Substance 3D Materials / Substance 3D Viewer (beta)** workflows for placing 3D objects.
  - Native **OpenColorIO / ACES** color management and better 32-bit HDR support.
  - Variable fonts.
  - https://news.adobe.com/news/2024/10/101424-new-innovations-in-photoshop-and-illustrator · https://www.cgchannel.com/2024/10/adobe-releases-photoshop-26-0-and-updates-photoshop-on-the-web/ · https://blog.adobe.com/en/publish/2024/10/14/adobe-max-2024-more-power-creators
- **26.1 (Nov 2024)**: Contextual Task Bar for gradients.
- **26.2 (Dec 2024)**: font filter UI changes.

### 2025
- **Jan 2025**: **Live Co-Editing** entered private beta (desktop beta and web, cloud documents only). No GA confirmed as of Sept 2026. https://blog.adobe.com/en/publish/2025/01/14/photoshop-unlocks-creative-collaboration-with-live-co-editing-join-private-beta
- **26.3 (Jan 2025)**: Frame tool shape options. Generate Image integration into frames.
- **26.4 (Feb 2025)**: performance and stability work.
- **Feb 25, 2025**: **Photoshop on iPhone** launched (free tier with layers, masks, Tap Select, Spot Heal, Generative Fill/Expand; "Photoshop Mobile & Web" plan at $7.99/mo or $69.99/yr). Photoshop on the web was bundled into the same plan. https://news.adobe.com/news/2025/02/photoshop-mobile-web · https://petapixel.com/2025/02/25/you-can-now-use-adobe-photoshop-on-your-iphone/
- **26.5 (Mar 2025)**: tabbed Adjustments panel and drag-and-drop preset organization.
- **26.6 (Apr 2025)**: **Select Details** (select hair, clothing, facial parts and similar), on-canvas color adjustment, **Composition Reference** for Generate Image, cloud processing for selections. Firefly Image 4 became available in Generate Image around this time.
- **June 2025**: **Photoshop on Android (beta)**. https://techcrunch.com/2025/06/03/adobe-launches-beta-version-of-its-photoshop-app-on-android/
- **26.8 (June 2025)**: **Dynamic Text** (auto-fit text layouts), cloud or on-device choice for Select Subject and Remove Background, **AVIF and JPEG XL** support with HDR.
- **26.9 (July 2025)**: Remove tool added to the Contextual Task Bar. Choice of Firefly model version. Harmonize in beta around this period.
- **Sept 25, 2025 (beta)**: Generative Fill partner models (Google Gemini 2.5 Flash Image, "Nano Banana"; Black Forest Labs FLUX.1 Kontext). https://blog.adobe.com/en/publish/2025/09/25/photoshop-beta-expands-generative-fillmore-ai-models-more-possibilities
- **26.10 (Aug 2025)**: Star tool customization.
- **26.11 (Sept 2025)**: **Projects** for organizing and sharing, Firefly image refinement, CJK typing improvements.
- **27.0 (Oct 28, 2025, "Photoshop 2026", MAX 2025)**:
  - **Generative Fill with partner models** GA: Gemini 2.5 Flash Image, FLUX.1 Kontext, plus Firefly.
  - **Generative Upscale** (Topaz Gigapixel and Bloom plus Firefly Upscaler, up to 4K, billed in generative credits).
  - **Harmonize** GA: matches light, color and shadow for composited objects.
  - Improved **Select Subject / Remove Background** (cloud and on-device).
  - **Color & Vibrance adjustment layer** with Temperature and Tint.
  - Adobe Express template access, Adobe Stock integration, Firefly image and video import.
  - **AI Assistant (agentic)** announced in private beta for the web.
  - **Project Moonlight** previewed as a cross-app agent.
  - https://news.adobe.com/news/2025/10/adobe-max-2025-creative-cloud · https://photoshopcafe.com/whats-new-in-photoshop-2026-full-release-overview/
- **27.1 (Nov 2025)**: stability fixes.
- **27.2 (Dec 2025)**: **FLUX.2 [pro]** partner model in Generative Fill.

### 2026
- **Jan 27, 2026 (27.2 per PetaPixel, 27.3 per VideoHelp)**:
  - New **Clarity** and **Dehaze** adjustment layers and a **Grain** adjustment layer (ACR tools as non-destructive, maskable layers).
  - Generative Fill, Expand and Remove use the new Firefly Fill & Expand model with **2K output** and fewer artifacts.
  - Upgraded Reference Image consistency.
  - **Dynamic Text beta** (arc and circle text).
  - https://petapixel.com/2026/01/27/photoshop-update-adds-popular-camera-raw-tools-as-adjustment-layers/ · https://blog.adobe.com/en/publish/2026/01/27/new-photoshop-innovations-provide-creative-pros-more-control-realism-precision
- **27.4 (Feb 2026)**: Generate Similar uses the updated Fill & Expand model.
- **Mar 10, 2026**: **Photoshop AI Assistant public beta** on **web and mobile only** (not desktop).
  - Text and voice (on mobile) instructions. It either performs the edit or walks you through the steps.
  - **AI Markup** on web: draw on the image, then prompt.
  - Firefly Image Editor.
  - 25+ partner models, including **Nano Banana 2, OpenAI image generation and FLUX.2**.
  - https://petapixel.com/2026/03/10/photographers-can-now-tell-photoshop-how-to-edit-their-images/ · https://techcrunch.com/2026/03/10/adobe-is-debuting-an-ai-assistant-for-photoshop/
- **27.5 (Mar 2026)**: **Firefly Boards integration**. Open cloud documents in Boards, generate variations, send them back.
- **Apr 2026**: Project Moonlight became the **Firefly AI Assistant**, a cross-app agent covering Photoshop, Lightroom, Premiere, Illustrator and Express, heading to public beta. https://blog.adobe.com/en/publish/2026/04/15/introducing-firefly-ai-assistant-new-way-create-with-our-creative-agent
- **27.6 (Apr 28, 2026)**:
  - **Rotate Object**: 3D-like re-angling of 2D objects, with no credits charged.
  - **Firefly Image Model 5** and **Gemini 3.1** partner model.
  - Multiple reference images and text-to-image inside the workspace.
  - **Generative Credits Usage panel**.
  - **AI Layer Cleanup**: removes empty layers and gives layers descriptive names.
  - Redesigned **Actions panel** with natural-language search.
  - Remove tool **"Find Distractions"** mode.
  - **Reflection Removal** in Photoshop.
  - Dynamic Text on curved paths, editable gradients after application, Filter Gallery color controls, AMD Zen 4 optimizations.
  - https://petapixel.com/2026/04/28/adobes-latest-photoshop-and-lightroom-updates-focus-on-speed/
- **May 2026**: hovering over Generate shows an **estimated credit cost**. About 10 credits for Firefly Image 5 Generative Fill versus about 40 for Imagen-class partner models. https://fstoppers.com/education/adobes-new-ai-credit-cost-preview-photoshop-what-need-know-902663
- **27.7 (May 2026)**: Firefly Boards with PSD/PSDC files. **On-device generative AI** option.
- **27.8 (June 2026)**: model picker in Generate Image (Firefly or partner models). Week of June 15: the **Remove tool runs generative removal on-device, offline**, and **Reflection Removal** now outputs reflections to a separate layer with adjustable opacity. https://www.photoworkout.com/adobe-june-2026-on-device-ai-photoshop-lightroom/
- **27.9 / 27.9.1 (July 2026)**: Remove tool Contextual Task Bar changes, multi-layer export to Firefly Boards, cloud file search.
- **27.10 (Aug 2026)**:
  - **Light adjustment layer** (Exposure, Contrast, Highlights, Shadows, Whites, Blacks; legacy Brightness/Contrast modes kept).
  - **AI Assisted Editor (beta)**: a simplified natural-language editing mode alongside the "Pro" editor.
  - **Prompt to Edit / Instruct Edit** in the Contextual Task Bar (Firefly Image 5).
  - **Masks and selections to constrain generative edits**.
  - **Markup** (arrows, circles, doodles) to guide AI, using Gemini / Nano Banana and Nano Banana Pro.
  - Dynamic Text for any shape or path.
  - Dockable **Adobe Stock panel** (900M+ assets).
  - Customizable Save As format list.
  - AMD Zen 4/5 performance work. Requires macOS 14+ or Windows 10+.
  - https://community.adobe.com/announcements-710/photoshop-27-10-new-ai-editing-tools-light-adjustment-layer-and-more-1639062 · https://petapixel.com/2026/08/27/adobe-brings-acrs-powerful-non-destructive-exposure-tools-into-photoshop-itself/

### Feature status checklist (Photoshop)
| Feature | Status Sept 2026 |
|---|---|
| Generative Fill / Expand / Remove | GA. Firefly Fill & Expand model (2K). Partner models: Gemini 2.5/3.1 (Nano Banana, Nano Banana 2/Pro), FLUX.1 Kontext, FLUX.2 pro, OpenAI |
| Generate Image / Generate Background / Generate Similar | GA (Firefly Image 5 plus partner models, multi-reference) |
| Harmonize | GA since Oct 2025 (no credit charge per Fstoppers) |
| Generative Upscale | GA (Firefly, Topaz Gigapixel/Bloom) |
| Distraction Removal / Find Distractions | GA (Remove tool). Generative remove can run on-device |
| Reflection Removal | GA in PS (Apr 2026), outputs a separate layer (June 2026) |
| Adjustment Brush, Selection Brush, Contextual Task Bar | GA |
| Object Select / Select Subject / Remove Background | Upgraded Oct 2025, cloud or on-device |
| Neural Filters | Still shipping. No notable updates found in 2024-2026. Adobe's investment has moved to Firefly and ACR |
| AI Assistant (agentic) | Public beta on web and mobile (Mar 2026). Desktop has the "AI Assisted Editor (beta)" (Aug 2026). Cross-app Firefly AI Assistant (ex-Project Moonlight) in beta |
| Generative Workspace | Beta (Oct 2024). Largely overtaken by Firefly Boards integration |
| Substance 3D materials / 3D Viewer | Beta workflows (since Oct 2024) |
| Live Co-Editing | Private beta since Jan 2025. No GA found |
| Content Credentials | Available on export (C2PA) |
| Adobe Express integration | Templates accessible from PS (Oct 2025) |
| New ACR-derived adjustment layers | Color & Vibrance with Temp/Tint (Oct 2025), Clarity, Dehaze, Grain (Jan 2026), Light (Aug 2026) |

### Adobe Camera Raw (feeds both PS and LrC)
- Lens Blur (EA Oct 2023, GA May 2024; oval/anamorphic bokeh Dec 2024).
- Point Color (Oct 2023; **Variance** slider Oct 2025).
- HDR editing and output (Oct 2023; ISO 21496-1 gain maps Oct 2024; HDR Limit slider 1.0 to 8.0 Oct 2025).
- AI Denoise (2023). Became a non-destructive Detail-panel feature with no separate DNG in June 2025 (ACR 17.4 / LrC 14.4). Supports ProRAW, Samsung Expert RAW and linear DNG (Oct 2024). Uses the Apple Neural Engine (June 2026).
- **Adaptive profiles**, Adaptive: Color and Adaptive: B&W (Feb 2025). Raw/DNG only, amount 0 to 200. https://community.adobe.com/t5/lightroom-classic-discussions/p-adaptive-profiles/m-p/15153993
- Generative Remove, Distraction Removal: People and Reflections (June 2025). Dust Removal (Oct 2025).
- Landscape masks (Apr 2025), Snow mask (Oct 2025).
- **ACR 18.6 (late Aug / Sep 1, 2026)**: **Glow** (Diffusion / Bloom / Halation, maskable; ACR-only for now), **Trim** (transparent outside the crop, works with Generative Expand), **Metadata Info panel**. https://www.dpreview.com/news/adobe-adds-glow-to-camera-raw/ · https://petapixel.com/2026/09/04/im-really-digging-adobe-camera-raws-new-glow-effect/
- Commentary (Thomas Fitzgerald): ACR now gets more new features than Lightroom Classic.

---

## 2. Lightroom Classic: chronology, mid-2024 to Sept 2026
Primary sources: https://www.lightroomqueen.com/whatsnew/classic/ and the per-release Lightroom Queen pages linked below.

- **13.3 (May 2024)**: **Generative Remove (Early Access)**, **Lens Blur GA** (batch/sync), and Sony tethering added. https://www.lightroomqueen.com/whats-new-in-lightroom-2024-05/
- **13.5 (Aug 13, 2024)**: Sync Activity reinstated, **HDR viewing in Library** (Loupe, Compare, full screen), Edit in Photoshop Beta option. https://www.lightroomqueen.com/whats-new-in-lightroom-2024-08/
- **14.0 (Oct 14, 2024)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2024-10/
  - **Generative Remove GA** with "Detect Objects".
  - **Limit Preview Cache Size**.
  - Denoise for ProRAW, Expert RAW, linear and HDR-merged files.
  - Shift-H toggles HDR.
  - ISO gain-map HDR export (JPG, JXL, AVIF, TIFF).
  - Streamlined catalog upgrade, **Rename Catalog**.
  - **Content Credentials (Early Access)** on export.
  - Native Apple Silicon Nikon tether.
  - Bulk Delete Empty Masks.
- **14.1 (Dec 12, 2024)**: Generative Remove works on areas outside the crop or transform. **Lens Blur oval/anamorphic bokeh** and a cat-eye slider. "Discard Standard and 1:1 Previews" command. **AVX2 CPU now required**. https://www.lightroomqueen.com/whats-new-in-lightroom-2024-12/
- **14.2 (Feb 13, 2025)**: **Adaptive Profiles** (Color and B&W). Tethering: click-to-focus-point in Live View for Sony, Canon and Nikon. **Catalog Backups tab**. https://www.lightroomqueen.com/whats-new-in-lightroom-2025-02/
- **14.3 (Apr 24, 2025)**: **Landscape masking** (sky, mountains, architecture, vegetation, water, artificial and natural ground). Transparency checkerboard. Remove catalogs from the Open Recent list. Reset prefs clears caches. https://www.lightroomqueen.com/whats-new-in-lightroom-2025-04/
- **14.4 (June 17, 2025)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2025-06/
  - **Denoise and Super Resolution in the Detail panel, with no separate DNG** (non-destructive).
  - **Distraction Removal: People** and **Reflections**.
  - Orange "AI settings need update" indicator.
  - Better duplicate-import detection.
  - XMP performance improvements.
  - **Fujifilm tethering natively**; Canon R1, R5 II and R50 V tethering.
- **14.5 (Aug 13, 2025)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2025-08/
  - Copy/Sync checkbox presets.
  - **GPU preview generation** (up to 2x faster).
  - Updated Generative Remove model.
  - More Sony tether models.
- **15.0 (Oct 28, 2025)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2025-10/
  - **Assisted Culling (EA)** and **Auto-Stacking by visual similarity**.
  - **Point Color Variance**.
  - **HDR Limit slider**.
  - Improved reflection removal.
  - **Dust Removal / dust spot detection**.
  - **Snow mask**.
  - Zoom while cropping (Z).
  - Filter by web likes and comments.
  - Separate SDR/HDR Edit in PS settings.
  - Milliseconds in capture time.
  - 4K video export.
  - Minimum OS: macOS 14 / Win10 22H2.
- **15.1 (Dec 16, 2025)**: **PSB support** (export and Edit in PS), better import previews, Leica tethering. https://www.lightroomqueen.com/whats-new-in-lightroom-2025-12/
- **15.2 (Feb 20, 2026)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2026-02/
  - **WebP import and edit**.
  - **Topaz Generative Upscale** at 2x/4x (credits).
  - "Generate using Firefly" and image-to-video handoff.
  - Batch Rename in the Export dialog.
  - Culling model tuned for groups.
- **15.3 (Apr 15, 2026)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2026-04/
  - **Background AI processing** for Copy/Paste/Sync.
  - Faster, more fluid sliders.
  - Sync supports PSB, with faster downloads.
  - Culling scoring refined.
  - Film-inspired presets and profiles.
  - Firefly mood boards.
  - Sony A7 V compressed raw.
- **15.4 (June 18, 2026; pulled, replaced by 15.4.1 for a data-loss bug)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2026-06/ · https://photoshopcafe.com/lightroom-classic-update-deep-dive-whats-new-and-what-actually-matters-june-2026/
  - **Assisted Culling GA** with a **Faces panel** (per-face Eyes Open / Eye Focus).
  - **Duplicate Detection** (AI, indexed, stacked). 15.4 was withdrawn because "Delete Rejected Photos" in Duplicates view lost data.
  - **Keyword sync** to the cloud ecosystem.
  - Improved Select Subject.
  - **Denoise on Apple Neural Engine**.
  - Faster masking brush.
  - Background activity indicator.
  - Topaz AI Sharpen (Noise-Aware Sharpen) appeared in the ecosystem; LRQ lists it as desktop-exclusive.
- **15.5 (Aug 3, 2026) / 15.5.1 (Aug 31)**: https://www.lightroomqueen.com/whats-new-in-lightroom-2026-08/ · https://blog.thomasfitzgeraldphotography.com/blog/2026/8/lightroom-15-5-released-new-features
  - **Feather and Edge sliders for AI masks** (not for brush masks).
  - **Render to DNG**: bakes edits into a linear/TIFF-in-DNG, stackable Denoise plus Super Res, about 2.5x the raw size.
  - Crop overlay opacity.
  - Rotate/Flip recorded in History.
  - Flatten AI Edits.
  - **Catalog (lrcat-data) corruption auto-detect and repair** on startup.
  - **Generative Expand did NOT come to Classic.** It is desktop and iPhone only.

### Status checklist (LrC)
| Feature | Status |
|---|---|
| Generative Remove | GA (Oct 2024). Model updated Aug 2025 |
| Distraction Removal (People / Reflections / Dust) | GA (June 2025 / Oct 2025) |
| AI Denoise non-destructive (no DNG) | GA June 2025. ANE-accelerated June 2026 |
| Adaptive Color / B&W profiles | GA Feb 2025 |
| Lens Blur | GA May 2024 |
| Point Color (+Variance) | GA (Variance Oct 2025) |
| AI masks: Subject, Sky, Background, Objects, People (with parts), Landscape (7 types), Snow | GA. Feather/Edge added Aug 2026 |
| Assisted Culling + Auto-stack + Faces | GA June 2026 |
| Duplicate Detection | GA June 2026 |
| Tethering | Canon, Nikon (native Apple Silicon), Sony (expanded 2024-25), Fujifilm (native June 2025), Leica (Dec 2025) |
| HDR editing and display | GA. HDR Limit slider. Gain-map export |
| Content Credentials | Export (Early Access label per Adobe docs) |
| Preview cache limit | Oct 2024 |
| Quick Actions | Not in Classic (mobile/web only) |
| Generative Expand, natural-language search, Generate Video dialog, interactive histogram, 10 custom color labels, Edit using Describe | Not in Classic |

### Roadmap statements
- Adobe has repeatedly said Classic is not being discontinued. It gets bimonthly releases on the same schedule as the cloud apps. There is commentary, including "Is Adobe trying to kill off Lightroom Classic?" videos after the Aug 2026 release, that the cloud apps and ACR now get features first or exclusively (Generative Expand, Glow, NL search). https://frameandfocal.com/post-processing/adobe-no-not-killing-lightroom-classic · https://blog.thomasfitzgeraldphotography.com/blog/2026/8/lightroom-15-5-released-new-features
- The claim that "the Apr 2023 roadmap guaranteed investment through 2027" comes from a secondary blog and was not verified in Adobe's own text.

---

## 3. Lightroom (cloud) desktop and mobile features that Classic lacks (Sept 2026)
- **Quick Actions** (mobile and web; iPad from Dec 2025): Subject/Skin/Teeth/Blur background, **Scene** (June 2025), Fix Angle, **Blemishes** (Oct 2025), one-tap skin retouch with manual touch-up (2026).
- **Generative Expand** (desktop 9.5 and iPhone, Aug 2026).
- **Animate / Photo to Video** (Firefly and Google Veo, credits). Integrated Generate Video dialog on desktop (June 2026).
- **Natural-language / AI visual search** in the cloud catalog (e.g. "houses with thatched roofs").
- **Edit using Describe** (text-prompt editing; Android first, Apr 2026).
- **Adaptive: Landscape presets** (Spring/Summer/Autumn/Winter) and other Adaptive presets (desktop and mobile).
- Interactive histogram with RGB readout and drag-to-adjust (desktop 9.4).
- 10 custom-named color labels (desktop 9.4).
- Topaz AI Sharpen handoff (desktop 9.4).
- Smart Albums (desktop 8.0), dual-monitor secondary window (8.2), Before/After in Compare.
- Video editing (a long-standing cloud-only feature; Classic can only trim, capture frames and apply limited presets).
- Web galleries, shared albums with contributor uploads, and Freemium sharing.
- Denoise on iPad (M1+) (Aug 2026).
- Pixel edits (Denoise, Gen Remove) stored in .acr sidecars (desktop 9.3).
- Features Classic has that cloud lacks: full catalog/folder management, Map/Book/Slideshow/Print/Web modules, publish services, plug-in SDK, tethering, Duplicate Detection, Render to DNG, and keyword hierarchy.

Sources: Lightroom Queen release pages above · https://fstoppers.com/lightroom/every-new-lightroom-feature-april-2026-901804

---

## 4. Pricing (US, Sept 2026)
| Plan | Price | Includes | Gen credits |
|---|---|---|---|
| **Lightroom (1TB)** | **$14.99/mo** annual-billed-monthly ($149.99/yr prepaid; $22.49 month-to-month) since **Mar 20, 2026**, up from $11.99. Adobe's signed-out page may still show $11.99 | Lightroom cloud apps only (no Classic, no PS) | 250/mo |
| **Photography (1TB)** | $19.99/mo annual ($239.88/yr) | LrC, Lightroom, Photoshop (desktop, web, mobile), 1TB | 1,000/mo (per Adobe plan page) |
| Photography (20GB) | Closed to new subscribers since Jan 15, 2025. Existing monthly-billed users went from $9.99 to $14.99; prepaid annual stays about $119.88 | LrC, Lr, PS, 20GB | legacy |
| Photoshop single app | ~$22.99 to $34.49/mo depending on source (CG Channel lists $34.49/mo or $263.88/yr) | PS | — |
| **Creative Cloud Pro** (formerly All Apps; renamed June 17, 2025 in the US) | $69.99/mo annual (promos at $34.99 for 3 months) | 20+ apps, 100GB, unlimited "standard" gen-AI features, premium Firefly/partner models | 4,000/mo premium credits |
| **Creative Cloud Standard** | ~$54.99/mo | All desktop apps, limited mobile/web premium | 25/mo |
| Photoshop Mobile & Web | $7.99/mo or $69.99/yr (free tier available) | PS iPhone, iPad, Android, web | — |
| Credit add-ons | 2,000 for $9.99, 7,000 for $29.99, 10,000 for $49.99, 50,000 for $199 per month | | |

- **Credit system:** Adobe splits AI features into "standard" and "premium". Standard features (Gen Fill with Firefly and similar) are unlimited on CC Pro, but premium features (partner models, video, 4K) consume credits. Costs vary by model: Firefly Image 5 Gen Fill is about 10 credits and partner models are up to about 40. There is a cost preview on hover (May 2026) and a Credits Usage panel (Apr 2026). Credits don't roll over. Denoise, Harmonize and Rotate Object do not consume credits (per Fstoppers).
- Sources: https://www.adobe.com/creativecloud/photography/compare-plans.html · https://photutorial.com/how-to-buy-lightroom/ · https://helpx.adobe.com/lightroom-cc/kb/lightroom-1tb-plan-faq.html · https://helpx.adobe.com/creative-cloud/faq/ccpp-20gb.html · https://photoshopcafe.com/generative-credits-to-be-enforced-adobe-cc-plans-change/ · https://petapixel.com/2025/06/24/adobe-is-now-tracking-generative-credit-use-what-you-need-to-know/ · https://fstoppers.com/education/adobes-new-ai-credit-cost-preview-photoshop-what-need-know-902663
- Caveat: the Photography 1TB credit number (1,000) comes from Adobe's page as fetched. Earlier (2025) it was lower, so verify at checkout.

---

## 5. Known gaps and common complaints

**Both apps / Adobe generally**
- Subscription only, with no perpetual license. Repeated price increases: Photography 20GB in Jan 2025, All Apps up 17% in 2025, Lightroom 1TB in Mar 2026. Mid-2026 press describes creatives moving to owned software (Affinity, Capture One perpetual, DxO, ON1, Luminar). https://medium.com/@aarosmith.cs/adobes-2026-price-hikes-are-pushing-creatives-back-toward-software-you-actually-own-2b695adc77a0
- Generative credit confusion and cost: partner models cost 4x Firefly, failed generations still consume credits, and users call credits "a money grab". https://fstoppers.com/artificial-intelligence/ai-photo-editing-credits-industrys-dirtiest-money-grab-900407 · https://mattk.com/are-generative-credits-getting-out-of-control/
- AI "bloat" and UI clutter. Many cloud-dependent features need an internet connection and sign-in. There are also concerns about content and privacy, which Adobe addressed with on-device Remove (June 2026).

**Photoshop**
- Performance regressions: 27.10 and 27.11 beta open/save >50% slower on Win 11. Frequent point-release crash fixes (26.6.1, 27.3.1). https://community.adobe.com/questions-712/multiple-issues-with-photoshop-27-10-1639899
- The AI Assistant launched on web and mobile first; desktop only has the beta "AI Assisted Editor".
- Live Co-Editing is still in private beta after 18+ months.
- Neural Filters have stagnated.
- Generative Fill has resolution limits (only 2K since Jan 2026) and inconsistent results.

**Lightroom Classic**
- Aging architecture: slowness with large catalogs (300k+ images), in Develop, and with 60MP+ files. The usual recommendations are 32GB+ RAM and NVMe. https://imagen-ai.com/valuable-tips/why-lightroom-classic-slow-fix/ · https://www.lightroomqueen.com/community/threads/what-is-the-upper-limit-for-size-of-a-catalog-bad-performance-issues.52487/
- Single-user SQLite catalog. No multi-user or network catalog. Catalog corruption risk (auto-repair only arrived in 15.5).
- **No layers**, limited compositing, and Feather/Edge only on AI masks (brush masks lack them).
- Denoise, Super Res and Gen Remove bloat storage. Render to DNG is about 2.5x raw size.
- Growing feature gap versus Lightroom cloud and ACR: Generative Expand, Glow, NL search, Quick Actions, Edit using Describe, interactive histogram.
- Quality incidents: 15.4 was pulled for a data-loss bug.
- AVX2 CPU and macOS 14+ requirements drop older machines.
- Content Credentials are still labelled Early Access in Classic docs.
- Tethering historically lagged, though Sony, Fuji and Leica support has improved since 2024.
