# B5-50 — Library and Develop identifier map

Interactive controls expose `library.<area>.<control>` or `develop.<area>.<control>`.
An accessible name is AXLabel or the standard AXTitle; a value, tooltip, or placeholder
alone is not a name. Shared library browsers retain their Library identifiers when
shown as the Develop filmstrip. The shell toolbar switches namespace with the workspace;
Document identifiers remain unchanged.

## Dynamic conventions

| Pattern | Meaning |
| --- | --- |
| `library.sidebar.row.<key>[.name,.disclosure]` | Library source or collection key; row, editable name, native disclosure |
| `library.thumbnail.{grid,filmstrip}.<photoID>` | Thumbnail cell; photo name, cull decision and status |
| `library.selection.<key>` | Decision, grade, mark or basket keyboard key |
| `library.metadata.<field>` | Editable IPTC/XMP field |
| `library.filter.<facet>` | Filter facet or named search/date action |
| `library.import.locate.<path>` / `library.import.resetRoot.<path>` | Root relocation actions |
| `library.import.mark.<label>` | Colour-label mapping |
| `develop.slider.<parameter.path>` | Numeric Develop parameter; stable engine JSON path |
| `develop.basic.<key>` | Basic adjustment key |
| `develop.panel.<name>` | Persisted inspector panel disclosure |
| `develop.grading.<range>.wheel` | Colour-grading wheel |
| `develop.masks.row.<groupID>[.visibility]` | Mask group selection and visibility |
| `develop.masks.component.<index>.<action>` | Selected mask component's combine, invert, retry or remove action |
| `develop.masks.component.{add,subtract,intersect}` | Component creation menus |
| `develop.masks.slider.<parameter>` | Selected mask's local adjustment |
| `develop.history.step.<stepID>.<action>` | History enable/restore action |
| `develop.history.group.<groupID>.<action>` | Agent history amount, instruction and redo actions |
| `develop.presets.{apply,delete}.<name>` | Saved preset, keyed by its store name |
| `develop.snapshots.restore.<name>` | Named snapshot restore |
| `<segment prefix>.<index>` | Segment in declared order, with its own accessible name |

`AccessibilityKey.component` percent-encodes model-derived names/paths where needed.
IDs do not use localized display names when a model key exists. Mask component indices
identify positions within the selected group. Preset/snapshot names are their store keys;
renaming changes those identifiers. Native toolbar wrappers can repeat the identifier of
the child representing the same action; distinct sibling controls cannot share an ID.

The Lightroom report's static groups are also identified:

| Identifier | Accessible content |
| --- | --- |
| `library.import.report.summary` | All eight report counts |
| `library.import.report.warnings` | Skipped photos and the **Not fully supported** group, including categories, reasons, counts and examples |
| `library.import.report.approximate` | **Approximate translations**, kept separate from warnings |
| `library.import.report.fidelity` | Renderer and sample diagnostics |
| `library.import.report.markdown` | Selectable, read-only report text |
| `library.import.report.disclosure` | Expand/collapse report markdown |

## Observed interactive controls

Collected by the background hosted audit with synthetic fixtures. This is an observed
map, including fixture-specific records and expanded conditional panels, rather than an
inventory of every possible user record. Additional annotated context-menu and popover
controls are described by the conventions above. System dialogs and transient menus are
not exhaustively opened by this test.

**325 distinct production identifiers** in the focused run.

| Identifier | Accessible name(s) |
| --- | --- |
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
| `develop.detail.aiDenoise` | AI Denoise |
| `develop.detail.aiDenoise-amount` | Amount |
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
| `develop.hdr.enabled` | HDR (extended dynamic range) |
| `develop.hdr.headroom` | Headroom |
| `develop.history.original` | Original |
| `develop.history.step.1.enabled` | Enable Edit |
| `develop.history.step.1.restore` | Restore Edit |
| `develop.hsl.property.0` | Hue |
| `develop.hsl.property.1` | Saturation |
| `develop.hsl.property.2` | Luminance |
| `develop.hsl.target` | Targeted adjustment: drag up/down on a colour in the loupe to change its hue / Targeted adjustment: drag up/down on a colour in the loupe to change its luminance / Targeted adjustment: drag up/down on a colour in the loupe to change its saturation |
| `develop.inspector.tabs.0` | Develop |
| `develop.inspector.tabs.1` | Masks |
| `develop.lensBlur.amount` | Blur Amount |
| `develop.lensBlur.apply` | Apply |
| `develop.lensBlur.bokehShape` | Bokeh shape |
| `develop.lensBlur.refine-blur` | Blur brush: paint areas to blur |
| `develop.lensBlur.refine-focus` | Focus brush: paint areas to keep sharp |
| `develop.lensBlur.subject` | Subject |
| `develop.lensBlur.visualize-depth` | Visualize Depth |
| `develop.loupe.displayInfo` | Display info |
| `develop.loupe.masks` | Masks |
| `develop.loupe.shortcuts` | Shortcuts |
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
| `develop.panel.Editingtarget` | Editing target |
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
| `develop.presets.apply.AX%20preset` | Apply preset AX preset |
| `develop.presets.new` | Save Preset… |
| `develop.proof.blackPoint` | Black point compensation |
| `develop.proof.chooseProfile` | Other… |
| `develop.proof.enabled` | Soft proofing |
| `develop.proof.gamut` | Gamut warning |
| `develop.proof.intent.0` | Perceptual |
| `develop.proof.intent.1` | Relative |
| `develop.proof.paper` | Simulate paper and ink |
| `develop.proof.profile` | Proof profile |
| `develop.proof.warningColor` | Gamut warning colour |
| `develop.slider.color.grading.balance` | Balance |
| `develop.slider.color.grading.blending` | Blending |
| `develop.slider.color.grading.global.hue` | Hue |
| `develop.slider.color.grading.global.luminance` | Global / Luminance |
| `develop.slider.color.grading.global.saturation` | Saturation |
| `develop.slider.color.grading.highlights.hue` | Hue |
| `develop.slider.color.grading.highlights.luminance` | Highlights / Luminance |
| `develop.slider.color.grading.highlights.saturation` | Saturation |
| `develop.slider.color.grading.midtones.hue` | Hue |
| `develop.slider.color.grading.midtones.luminance` | Luminance / Midtones |
| `develop.slider.color.grading.midtones.saturation` | Saturation |
| `develop.slider.color.grading.shadows.hue` | Hue |
| `develop.slider.color.grading.shadows.luminance` | Luminance / Shadows |
| `develop.slider.color.grading.shadows.saturation` | Saturation |
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
| `develop.snapshots.restore.AX%20snapshot` | AX snapshot |
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
| `develop.toolbar.review` | Review 0 |
| `develop.toolbar.sidebar` | Show Sidebar |
| `develop.toolbar.workspace.0` | Library |
| `develop.toolbar.workspace.1` | Edit photo |
| `develop.toolbar.workspace.2` | Review |
| `develop.transform.aspect` | Aspect |
| `develop.transform.constrain-crop` | Constrain Crop |
| `develop.transform.horizontal` | Horizontal |
| `develop.transform.offset-x` | Offset X |
| `develop.transform.offset-y` | Offset Y |
| `develop.transform.reset` | Reset |
| `develop.transform.rotate` | Rotate |
| `develop.transform.scale` | Scale |
| `develop.transform.upright-auto` | Upright Auto |
| `develop.transform.upright-full` | Upright Full |
| `develop.transform.upright-guided` | Upright Guided |
| `develop.transform.upright-level` | Upright Level |
| `develop.transform.upright-off` | Upright Off |
| `develop.transform.upright-reset` | Reset |
| `develop.transform.upright-vertical` | Upright Vertical |
| `develop.transform.vertical` | Vertical |
| `develop.workspace.library` | Back to Library |
| `library.agent.autoEdit` | Auto Edit… |
| `library.assist.assist-panel-toggle` | Assist |
| `library.assist.mode.0` | Assisted |
| `library.assist.mode.1` | Automated |
| `library.empty.openFolder` | Open Folder… |
| `library.filter.album` | Album |
| `library.filter.camera` | Camera |
| `library.filter.date` | Date |
| `library.filter.decision` | Decision |
| `library.filter.grade` | Grade |
| `library.filter.keyword` | Keyword |
| `library.filter.lens` | Lens |
| `library.filter.mark` | Mark |
| `library.filter.person` | Person |
| `library.filter.rule` | Search or filter rule |
| `library.filter.save` | Save as Smart Album… |
| `library.import.back` | Back |
| `library.import.cancel` | Cancel |
| `library.import.chooseAnother` | Choose Another… |
| `library.import.chooseCatalog` | Choose Catalog… |
| `library.import.chooseLibraryFolder` | Choose… |
| `library.import.continue` | Continue |
| `library.import.done` | Done |
| `library.import.fidelitySort` | Fidelity sort order |
| `library.import.locate.%2FSynthetic%20Old` | Locate… |
| `library.import.looks-different` | Looks different (0) |
| `library.import.mark.Client%20choice` | Map colour label Client choice |
| `library.import.overwrite` | Replace edits already made in Tessera |
| `library.import.preview` | Preview Fidelity |
| `library.import.report.disclosure` | Import report markdown |
| `library.import.report.markdown` | Import report markdown |
| `library.import.resetRoot.%2FSynthetic%20Old` | Reset |
| `library.import.resume` | Resume Import |
| `library.import.showReport` | Show Report in Finder |
| `library.import.start` | Import 0 Photos |
| `library.inspector.editPhoto` | Edit photo |
| `library.keywords.entry` | Add keywords |
| `library.keywords.new` | New… |
| `library.metadata.altText` | Alt text |
| `library.metadata.caption` | Caption |
| `library.metadata.copyright` | Copyright |
| `library.metadata.creator` | Creator |
| `library.metadata.keywords` | Keywords |
| `library.metadata.title` | Title |
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
| `library.sidebar.add` | Add |
| `library.sidebar.row.basket:Selects` | Selects |
| `library.sidebar.row.folder:current` | No folder open / photos |
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
| `library.toolbar.assist` | Assist |
| `library.toolbar.autoAdvance` | Auto-advance |
| `library.toolbar.autoEdit` | Auto Edit |
| `library.toolbar.inspector` | Inspector |
| `library.toolbar.open` | Open Folder… |
| `library.toolbar.review` | Review 0 |
| `library.toolbar.sidebar` | Hide Sidebar |
| `library.toolbar.thumbnailSize` | Thumbnail size |
| `library.toolbar.view.0` | Grid |
| `library.toolbar.view.1` | Loupe |
| `library.toolbar.view.2` | Compare |
| `library.toolbar.workspace.0` | Library |
| `library.toolbar.workspace.1` | Edit photo |
| `library.toolbar.workspace.2` | Review |
| `library.understanding.keyword-suggest-selection` | Suggest |
| `library.understanding.metadata-generate-caption` | Generate |
| `library.understanding.ocr-detect` | Detect Text |
| `library.workspace.editPhoto` | Edit photo |

## Refresh

```sh
cd apps/mac
TESSERA_AX_MAP=1 swift test -c release -Xswiftc -enable-testing \
  --filter LibraryDevelopAccessibilityTests > /tmp/B5-50-map.stdout 2> /tmp/B5-50-map.stderr
```

Each complete `AX MAP` row contains the control identifier and accessible name. Deduplicate
repeated observations across scenarios when updating this table.
