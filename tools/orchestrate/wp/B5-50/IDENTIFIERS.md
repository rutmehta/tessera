# B5-50b — Library and Develop identifier map

Established identifiers are preserved. New controls use `library.<area>.<control>` or
`develop.<area>.<control>`. Shared controls retain their existing names in every workspace.
Document toolbar identifiers are documented in [B5-42](../B5-42/IDENTIFIERS.md).
See [the restoration inventory](RESTORED-IDENTIFIERS.md) for every corrected rename.

An accessible name is AXLabel or AXTitle; values, help and placeholders alone are not names.
The native Library/Develop sidebar toggle is provided by SwiftUI/AppKit and has no app identifier.

## Dynamic conventions and privacy

| Pattern | Meaning |
| --- | --- |
| `library.sidebar.row.<key>[.name,.disclosure]` | Static source key or numeric collection key; folder/basket rows use an opaque UUID |
| `library.thumbnail.{grid,filmstrip}.<photoID>` | Numeric photo ID |
| `library.selection.<key>` | Fixed decision, grade, mark or basket keyboard key |
| `iptc-<field>` | Established editable metadata field |
| `facet<Title>`, `facetPerson`, `facetAlbum` | Established facet names |
| `lrimport-locate` | Established locate action |
| `library.import.resetRoot.<index>` | Root index, never a catalog path |
| `lrimport-mark-<index>` | Mark-mapping row index, never its label |
| `develop.presets.{apply,delete}.<index>` | Preset position in the displayed model |
| `develop.snapshots.restore.<index>` | Snapshot position |
| `develop.basic.restoreSnapshot.<index>` | Snapshot menu position |
| `library.keywords.{remove,add,removeTree}.<index>` | Keyword position within the corresponding displayed list |
| `keyword-suggestion-<index>`, `keyword-suggestion-reject-<index>` | Suggestion position; established prefixes preserved |
| `develop.slider.<parameter.path>` | Stable engine parameter path |
| `develop.basic.<key>` | Basic adjustment key |
| `develop.panel.<name>` | Fixed inspector disclosure name; `editingTarget` uses lower camel case |
| `develop.grading.<range>.wheel` | Fixed colour-grading range |
| `develop.masks.row.<groupID>[.visibility]` | Mask model ID |
| `develop.masks.component.<index>.<action>` | Selected mask component position |
| `develop.masks.slider.<parameter>` | Local adjustment parameter |
| `agent-step-toggle-<stepID>` | Established history step toggle |
| `develop.history.group.<groupID>.submit` | Agent redo submission |
| `<segment prefix>.<index>` | Segment in declared order |

Names and paths belong in accessible labels, never in identifiers. Percent encoding is not
an anonymization mechanism. `AccessibilityKey.component` accepts only non-private keys.
Indexes track model order; opaque UUIDs persist for the row model lifetime. Built-in tone curve
preset names are fixed application constants. Native toolbar parent/child wrappers may repeat
one action's identifier. Established repeated group identifiers remain compatible with main.

## Import report and cancellation

| Identifier | Content/action |
| --- | --- |
| `document.import.report.summary` | Report counts |
| `document.import.report.warnings` | Skipped and unsupported entries |
| `document.import.report.approximate` | Approximate translations |
| `document.import.report.fidelity` | Renderer and sample diagnostics |
| `document.import.report.markdown` | Read-only report text |
| `library.import.report.disclosure` | Expand report markdown |
| `library.import.cancel` | Dismiss import sheet |
| `lrimport-cancel` | Cancel running import |

## Observed controls

Offscreen hosted audit with synthetic fixtures. This is not an exhaustive inventory of
system menus or transient popovers. UUID rows below are represented as `<opaqueID>`.

327 distinct observed identifiers before UUID normalization.

| Identifier | Accessible name |
| --- | --- |
| `agent-step-toggle-1` | Enable Edit |
| `assist-panel-toggle` | Assist |
| `detail-ai-denoise` | AI Denoise |
| `detail-ai-denoise-amount` | Amount |
| `develop.basic.blacks` | Blacks |
| `develop.basic.clarity` | Clarity |
| `develop.basic.contrast` | Contrast |
| `develop.basic.dehaze` | Dehaze |
| `develop.basic.exposure` | Exposure |
| `develop.basic.highlights` | Highlights |
| `develop.basic.reset` | Reset |
| `develop.basic.saturation` | Saturation |
| `develop.basic.shadows` | Shadows |
| `develop.basic.snapshots` | Snapshots |
| `develop.basic.temperature` | Temperature |
| `develop.basic.texture` | Texture |
| `develop.basic.tint` | Tint |
| `develop.basic.vibrance` | Vibrance |
| `develop.basic.whites` | Whites |
| `develop.crop.angle` | Straighten angle |
| `develop.crop.aspect` | Crop aspect ratio |
| `develop.crop.begin` | Crop & Straighten |
| `develop.crop.cancel` | Cancel |
| `develop.crop.constrain` | Constrain to image |
| `develop.crop.done` | Done |
| `develop.crop.orientation` | Swap landscape / portrait (X) |
| `develop.crop.overlay` | Crop overlay |
| `develop.crop.reset` | Reset |
| `develop.crop.straighten` | Straighten: draw along a horizon or vertical in the loupe |
| `develop.detail.previewArea` | Choose the 1:1 preview area in the loupe |
| `develop.effects.style` | Vignette style |
| `develop.grading.global.wheel` | Global |
| `develop.grading.highlights.wheel` | Highlights |
| `develop.grading.midtones.wheel` | Midtones |
| `develop.grading.mode.0` | 3-Way |
| `develop.grading.mode.1` | Shadows |
| `develop.grading.mode.2` | Midtones |
| `develop.grading.mode.3` | Highlights |
| `develop.grading.mode.4` | Global |
| `develop.grading.shadows.wheel` | Shadows |
| `develop.history.original` | Original |
| `develop.history.step.1.restore` | Restore Edit |
| `develop.hsl.property.0` | Hue |
| `develop.hsl.property.1` | Saturation |
| `develop.hsl.property.2` | Luminance |
| `develop.hsl.target` | Targeted adjustment: drag up/down on a colour in the loupe to change its luminance |
| `develop.inspector.tabs.0` | Develop |
| `develop.inspector.tabs.1` | Masks |
| `develop.lensBlur.bokehShape` | Bokeh shape |
| `develop.loupe.masks` | Masks |
| `develop.maskToolbar.ai.background` | Background |
| `develop.maskToolbar.ai.sky` | Sky |
| `develop.maskToolbar.ai.subject` | Subject |
| `develop.maskToolbar.brush.feather` | Feather |
| `develop.maskToolbar.brush.flow` | Flow |
| `develop.maskToolbar.brush.size` | Size |
| `develop.maskToolbar.done` | Done |
| `develop.maskToolbar.overlay` | Show the mask overlay (O) |
| `develop.maskToolbar.overlayColor` | Overlay colour |
| `develop.maskToolbar.tool.brush` | Brush — Paint on the photo · ⌥ erases · [ ] size · ⇧[ ] feather |
| `develop.maskToolbar.tool.colorRange` | Color Range — Click a colour · ⇧-click adds a sample · ⌥ subtracts |
| `develop.maskToolbar.tool.linear` | Linear Gradient — Drag from full effect to none · ⌥ subtracts from the mask |
| `develop.maskToolbar.tool.luminanceRange` | Luminance Range — Click a tone to select its brightness range · ⌥ subtracts |
| `develop.maskToolbar.tool.object` | Objects — Click an object or drag a box around it · ⌥ subtracts |
| `develop.maskToolbar.tool.person` | People — Drag a box around a face |
| `develop.maskToolbar.tool.radial` | Radial Gradient — Drag from the centre outwards · ⌥ subtracts |
| `develop.masks.amount` | Amount |
| `develop.masks.component.0.invert` | Invert this component |
| `develop.masks.component.0.remove` | Remove this component |
| `develop.masks.component.1.combine` | Component combination mode |
| `develop.masks.component.1.invert` | Invert this component |
| `develop.masks.component.1.remove` | Remove this component |
| `develop.masks.component.add` | Add component to selected mask |
| `develop.masks.component.intersect` | Intersect component with selected mask |
| `develop.masks.component.subtract` | Subtract component from selected mask |
| `develop.masks.create` | Create mask |
| `develop.masks.invert` | Invert |
| `develop.masks.reset` | Reset Sliders |
| `develop.masks.row.0.visibility` | Hide this mask's adjustments |
| `develop.masks.showTools` | Show mask tools |
| `develop.masks.slider.blacks` | Blacks |
| `develop.masks.slider.clarity` | Clarity |
| `develop.masks.slider.contrast` | Contrast |
| `develop.masks.slider.dehaze` | Dehaze |
| `develop.masks.slider.exposure` | Exposure |
| `develop.masks.slider.highlights` | Highlights |
| `develop.masks.slider.hue` | Hue |
| `develop.masks.slider.moire` | Moiré |
| `develop.masks.slider.noise` | Noise |
| `develop.masks.slider.saturation` | Saturation |
| `develop.masks.slider.shadows` | Shadows |
| `develop.masks.slider.sharpness` | Sharpness |
| `develop.masks.slider.temperature` | Temp |
| `develop.masks.slider.texture` | Texture |
| `develop.masks.slider.tint` | Tint |
| `develop.masks.slider.whites` | Whites |
| `develop.panel.Basic` | Basic |
| `develop.panel.ColorGrading` | Color Grading |
| `develop.panel.CropStraighten` | Crop & Straighten |
| `develop.panel.Detail` | Detail |
| `develop.panel.Effects` | Effects |
| `develop.panel.HDR` | HDR |
| `develop.panel.HSLColor` | HSL / Color |
| `develop.panel.Histogram` | Histogram |
| `develop.panel.History` | History |
| `develop.panel.LensBlur` | Lens Blur |
| `develop.panel.Presets` | Presets |
| `develop.panel.Snapshots` | Snapshots |
| `develop.panel.SoftProofing` | Soft Proofing |
| `develop.panel.ToneCurve` | Tone Curve |
| `develop.panel.Transform` | Transform |
| `develop.panel.editingTarget` | Editing target |
| `develop.presets.apply.0` | Apply preset AXPrivatePreset731 |
| `develop.presets.new` | Save Preset… |
| `develop.proof.blackPoint` | Black point compensation |
| `develop.proof.chooseProfile` | Other… |
| `develop.proof.gamut` | Gamut warning |
| `develop.proof.intent.0` | Perceptual |
| `develop.proof.intent.1` | Relative |
| `develop.proof.paper` | Simulate paper and ink |
| `develop.proof.profile` | Proof profile |
| `develop.proof.warningColor` | Gamut warning colour |
| `develop.slider.color.grading.balance` | Balance |
| `develop.slider.color.grading.blending` | Blending |
| `develop.slider.color.grading.global.hue` | Hue |
| `develop.slider.color.grading.global.luminance` | Luminance |
| `develop.slider.color.grading.global.saturation` | Saturation |
| `develop.slider.color.grading.highlights.hue` | Hue |
| `develop.slider.color.grading.highlights.luminance` | Luminance |
| `develop.slider.color.grading.highlights.saturation` | Saturation |
| `develop.slider.color.grading.midtones.hue` | Hue |
| `develop.slider.color.grading.midtones.luminance` | Luminance |
| `develop.slider.color.grading.midtones.saturation` | Saturation |
| `develop.slider.color.grading.shadows.hue` | Hue |
| `develop.slider.color.grading.shadows.luminance` | Luminance |
| `develop.slider.color.hsl.hue.aqua` | Aqua |
| `develop.slider.color.hsl.hue.blue` | Blue |
| `develop.slider.color.hsl.hue.green` | Green |
| `develop.slider.color.hsl.hue.magenta` | Magenta |
| `develop.slider.color.hsl.hue.orange` | Orange |
| `develop.slider.color.hsl.hue.purple` | Purple |
| `develop.slider.color.hsl.hue.red` | Red |
| `develop.slider.color.hsl.hue.yellow` | Yellow |
| `develop.slider.color.hsl.luminance.aqua` | Aqua |
| `develop.slider.color.hsl.luminance.blue` | Blue |
| `develop.slider.color.hsl.luminance.green` | Green |
| `develop.slider.color.hsl.luminance.magenta` | Magenta |
| `develop.slider.color.hsl.luminance.orange` | Orange |
| `develop.slider.color.hsl.luminance.purple` | Purple |
| `develop.slider.color.hsl.luminance.red` | Red |
| `develop.slider.color.hsl.luminance.yellow` | Yellow |
| `develop.slider.color.hsl.saturation.aqua` | Aqua |
| `develop.slider.color.hsl.saturation.blue` | Blue |
| `develop.slider.color.hsl.saturation.green` | Green |
| `develop.slider.color.hsl.saturation.magenta` | Magenta |
| `develop.slider.color.hsl.saturation.orange` | Orange |
| `develop.slider.color.hsl.saturation.purple` | Purple |
| `develop.slider.color.hsl.saturation.red` | Red |
| `develop.slider.color.hsl.saturation.yellow` | Yellow |
| `develop.slider.detail.noise_reduction.color` | Color |
| `develop.slider.detail.noise_reduction.color_detail` | Color Detail |
| `develop.slider.detail.noise_reduction.color_smoothness` | Smoothness |
| `develop.slider.detail.noise_reduction.luminance` | Luminance |
| `develop.slider.detail.noise_reduction.luminance_contrast` | Contrast |
| `develop.slider.detail.noise_reduction.luminance_detail` | Detail |
| `develop.slider.detail.sharpening.amount` | Amount |
| `develop.slider.detail.sharpening.detail` | Detail |
| `develop.slider.detail.sharpening.masking` | Masking |
| `develop.slider.detail.sharpening.radius` | Radius |
| `develop.slider.effects.grain.amount` | Amount |
| `develop.slider.effects.grain.roughness` | Roughness |
| `develop.slider.effects.grain.size` | Size |
| `develop.slider.effects.vignette.amount` | Amount |
| `develop.slider.effects.vignette.feather` | Feather |
| `develop.slider.effects.vignette.highlights` | Highlights |
| `develop.slider.effects.vignette.midpoint` | Midpoint |
| `develop.slider.effects.vignette.roundness` | Roundness |
| `develop.slider.tone.curves.parametric.darks` | Darks |
| `develop.slider.tone.curves.parametric.highlights` | Highlights |
| `develop.slider.tone.curves.parametric.lights` | Lights |
| `develop.slider.tone.curves.parametric.shadows` | Shadows |
| `develop.snapshots.new` | New Snapshot… |
| `develop.snapshots.restore.0` | AXPrivateSnapshot731 |
| `develop.tone.channel.0` | RGB |
| `develop.tone.channel.1` | R |
| `develop.tone.channel.2` | G |
| `develop.tone.channel.3` | B |
| `develop.tone.channel.4` | L |
| `develop.tone.mode.0` | Parametric |
| `develop.tone.mode.1` | Point |
| `develop.tone.presets` | Curve Presets |
| `develop.tone.reset` | Reset RGB |
| `develop.toolbar.inspector` | Inspector |
| `develop.toolbar.open` | Open Folder… |
| `develop.toolbar.workspace.0` | Library |
| `develop.toolbar.workspace.1` | Edit photo |
| `develop.toolbar.workspace.2` | Review |
| `develop.transform.reset` | Reset |
| `develop.transform.upright-reset` | Reset |
| `document.import.report.markdown` | Import report markdown |
| `facetAlbum` | Album |
| `facetCamera` | Camera |
| `facetDecision` | Decision |
| `facetGrade` | Grade |
| `facetKeyword` | Keyword |
| `facetLens` | Lens |
| `facetMark` | Mark |
| `facetPerson` | Person |
| `hdr-headroom` | Headroom |
| `hdr-toggle` | HDR (extended dynamic range) |
| `iptc-altText` | Alt text |
| `iptc-caption` | Caption |
| `iptc-copyright` | Copyright |
| `iptc-creator` | Creator |
| `iptc-keywords` | Keywords |
| `iptc-title` | Title |
| `keyword-suggest-selection` | Suggest |
| `keyword-suggestion-0` | Accept AXPrivateKeyword731 |
| `keyword-suggestion-reject-0` | Reject AXPrivateKeyword731 |
| `keyword-suggestion-threshold` | Accept-all threshold |
| `keyword-suggestions-accept-all` | Accept 1 |
| `keywordEntry` | Add keywords |
| `lensblur-amount` | Blur Amount |
| `lensblur-apply` | Apply |
| `lensblur-refine-blur` | Blur brush: paint areas to blur |
| `lensblur-refine-focus` | Focus brush: paint areas to keep sharp |
| `lensblur-subject` | Subject |
| `lensblur-visualize-depth` | Visualize Depth |
| `library.agent.autoEdit` | Auto Edit… |
| `library.assist.mode.0` | Assisted |
| `library.assist.mode.1` | Automated |
| `library.empty.openFolder` | Open Folder… |
| `library.filter.date` | Date |
| `library.filter.save` | Save as Smart Album… |
| `library.import.back` | Back |
| `library.import.cancel` | Cancel |
| `library.import.chooseAnother` | Choose Another… |
| `library.import.chooseCatalog` | Choose Catalog… |
| `library.import.chooseLibraryFolder` | Choose… |
| `library.import.continue` | Continue |
| `library.import.done` | Done |
| `library.import.fidelitySort` | Fidelity sort order |
| `library.import.overwrite` | Replace edits already made in Tessera |
| `library.import.preview` | Preview Fidelity |
| `library.import.report.disclosure` | Import report markdown |
| `library.import.resetRoot.0` | Reset |
| `library.import.resume` | Resume Import |
| `library.import.showReport` | Show Report in Finder |
| `library.import.start` | Import 0 Photos |
| `library.inspector.editPhoto` | Edit photo |
| `library.keywords.new` | New… |
| `library.keywords.remove.0` | Remove AXPrivateKeyword731 |
| `library.panel.AgentEdit` | Agent Edit |
| `library.panel.Assist` | Assist |
| `library.panel.Image` | Image |
| `library.panel.Keywords` | Keywords |
| `library.panel.Metadata` | Metadata |
| `library.panel.People` | People |
| `library.panel.PhotoEdit` | Photo Edit |
| `library.panel.Selection` | Selection |
| `library.selection.1` | Keep, key 1 |
| `library.selection.2` | Good, key 2 |
| `library.selection.3` | Best, key 3 |
| `library.selection.6` | Key 6 |
| `library.selection.7` | Key 7 |
| `library.selection.8` | Key 8 |
| `library.selection.9` | Key 9 |
| `library.selection.B` | Selects, key B |
| `library.selection.P` | Keep, key P |
| `library.selection.U` | Undecided, key U |
| `library.selection.X` | Reject, key X |
| `library.sidebar.row.<opaqueID>` | Selects |
| `library.sidebar.row.folder:current` | photos |
| `library.sidebar.row.hdr:albums` | Albums |
| `library.sidebar.row.hdr:cull` | Culling |
| `library.sidebar.row.hdr:folders` | Folders |
| `library.sidebar.row.hdr:library` | Library |
| `library.sidebar.row.src:all` | All Photos |
| `library.sidebar.row.src:decision:keep` | Keeps |
| `library.sidebar.row.src:decision:reject` | Rejects |
| `library.sidebar.row.src:decision:undecided` | Undecided |
| `library.sidebar.row.src:mark:6` | Needs Retouch |
| `library.sidebar.row.src:mark:7` | Client Favourite |
| `library.sidebar.row.src:mark:8` | Print |
| `library.sidebar.row.src:mark:9` | Review |
| `library.sidebar.row.src:people` | People |
| `library.sidebar.row.src:unfiled` | Not in Any Album |
| `library.toolbar.autoAdvance` | Auto-advance |
| `library.toolbar.inspector` | Inspector |
| `library.toolbar.open` | Open Folder… |
| `library.toolbar.thumbnailSize` | Thumbnail size |
| `library.toolbar.view.0` | Grid |
| `library.toolbar.view.1` | Loupe |
| `library.toolbar.view.2` | Compare |
| `library.toolbar.workspace.0` | Library |
| `library.toolbar.workspace.1` | Edit photo |
| `library.toolbar.workspace.2` | Review |
| `loupe-display-info` | Display info |
| `loupe-shortcuts` | Shortcuts |
| `lrimport-locate` | Locate… |
| `lrimport-looks-different` | Looks different (0) |
| `lrimport-mark-0` | Map colour label Client choice |
| `metadata-generate-caption` | Gendevelop original L2 calibration [first, tone, WB] ms: CPU [26.227584, 0.520875, 1.1101660000000002]; GPU [37.209917000000004, 1.258833, 1.328959] |
| `ocr-detect` | Detect Text |
| `ruleTextField` | Search or filter rule |
| `sidebarAddMenu` | Add |
| `softproof-toggle` | Soft proofing |
| `toolbar-agent-review` | Review 0 |
| `toolbar-assist` | Assist |
| `toolbar-auto-edit` | Auto Edit |
| `transform-aspect` | Aspect |
| `transform-constrain-crop` | Constrain Crop |
| `transform-horizontal` | Horizontal |
| `transform-offset-x` | Offset X |
| `transform-offset-y` | Offset Y |
| `transform-rotate` | Rotate |
| `transform-scale` | Scale |
| `transform-upright-auto` | Upright Auto |
| `transform-upright-full` | Upright Full |
| `transform-upright-guided` | Upright Guided |
| `transform-upright-level` | Upright Level |
| `transform-upright-off` | Upright Off |
| `transform-upright-vertical` | Upright Vertical |
| `transform-vertical` | Vertical |
| `workspace-back-to-library` | Back to Library |
| `workspace-edit-photo` | Edit photo |
