# Document identifier map — B5-42

Interactive controls use `document.<area>.<control>`. Accessible names are AXLabel/AXDescription or the standard AXTitle. Noninteractive containers can retain two-component identifiers.

## Dynamic conventions

| Pattern | Meaning / accessible name |
| --- | --- |
| `document.tool.<rawValue>` | Tool enum case; tool title and shortcut |
| `document.inspector.shortcut.<tab>` | Inspector tab shortcut; tab title |
| `document.layers.row.<index>` | Realized outline row; layer/filter/effect name |
| `document.layers.row.<index>.{name,visibility,maskLink,disclosure}` | Layer name / Show or hide layer / Link or unlink layer mask / Expand or collapse row |
| `document.layers.smartFilter.<layerID>.<index>.*` | Smart-filter actions; filter-specific labels |
| `document.layers.effect.<layerID>.<index>.*` | Layer-effect actions; effect-specific labels |
| `document.channels.<id>[.<action>]` | Channel model ID; name, eye, rename, swatch |
| `document.history.row.<index>` | History step; operation title |
| `document.history.snapshot.<index>.restore` | Snapshot restore; snapshot name |
| `document.tabs.<index>[.close]` | Document tab / Close document |
| `document.brushes.preset.<id>` | Brush preset; preset name |
| `<document segment prefix>.<index>` | Zero-based segment in declared order; title or help for an icon-only segment |

Layer/tab/history row indices identify positions, matching the existing app convention; reordering changes the positional identifier. Channel/preset IDs identify model records. Numeric segment IDs remain stable while their declared order remains stable. Duplicate sibling IDs fail; a native toolbar wrapper and its child may identify the same action.

## Observed controls

Collected from the passing hosted audit. These include fixture-specific records and tested conditional states, not every transient menu, popover or system dialog. Reuse across mutually exclusive scenarios is expected.

| Identifier | Accessible name(s) |
| --- | --- |
| `document.brush.angle` | Angle |
| `document.brush.hardness` | Hardness |
| `document.brush.roundness` | Roundness |
| `document.brush.size` | Size |
| `document.brush.spacing` | Spacing |
| `document.brushes.import` | Import Brushes… |
| `document.brushes.preset.builtin:chalk` | Chalk |
| `document.brushes.preset.builtin:square` | Square |
| `document.brushes.preset.round:0` | Soft Round |
| `document.brushes.preset.round:0.5` | Medium Round |
| `document.brushes.preset.round:1` | Hard Round |
| `document.brushes.toggle` | Brushes |
| `document.cam.color` | Default |
| `document.cam.mode.0` | Move |
| `document.cam.mode.1` | Extend |
| `document.cam.seed` | Seed |
| `document.cam.structure` | Structure |
| `document.channels.add` | New channel |
| `document.channels.channel.1.eye` | Show Alpha 1 |
| `document.channels.channel.1.rename` | Channel name |
| `document.channels.component.0.eye` | Hide Red |
| `document.channels.component.1.eye` | Hide Green |
| `document.channels.component.2.eye` | Hide Blue |
| `document.channels.delete` | Delete the highlighted channel |
| `document.channels.load` | Load the highlighted channel as a selection (⌘-click a channel) |
| `document.channels.quickMask` | Quick Mask mode (Q) |
| `document.channels.rgb.eye` | Hide RGB |
| `document.channels.save` | Save the selection as a new channel |
| `document.color.background` | Background |
| `document.color.default` | Default (D) |
| `document.color.foreground` | Foreground |
| `document.color.swap` | Swap (X) |
| `document.color.toggle` | Color |
| `document.colors.default` | Default colours (D) |
| `document.colors.swap` | Swap colours (X) |
| `document.history.height.decrease` | Decrease History height |
| `document.history.height.increase` | Increase History height |
| `document.history.height.reset` | Reset History height |
| `document.history.newSnapshot` | New Snapshot… |
| `document.history.row.0` | Opened |
| `document.history.row.1` | New Layer Group 1 |
| `document.history.row.2` | New Layer Layer 2 |
| `document.history.row.3` | Add Layer Mask |
| `document.history.row.4` | Drop Shadow |
| `document.history.row.5` | Convert to Smart Object |
| `document.history.row.6` | Gaussian Blur |
| `document.history.row.7` | New Channel |
| `document.history.snapshot.0.restore` | Restore |
| `document.history.toggle` | History |
| `document.inspector.tabs.0` | Stack |
| `document.inspector.tabs.1` | Properties |
| `document.inspector.tabs.2` | Channels |
| `document.layers.add` | New layer, group or fill layer |
| `document.layers.addAdjustment` | New adjustment layer |
| `document.layers.addMask` | Add a layer mask (reveal all; from the selection when there is one) |
| `document.layers.addStyle` | Add a layer style |
| `document.layers.blendMode` | Blend mode |
| `document.layers.delete` | Delete the selected layers (⌫) |
| `document.layers.effect.3.0.visibility` | Hide |
| `document.layers.fill` | Fill |
| `document.layers.group` | Group the selected layers (⌘G) |
| `document.layers.lock.all` | Lock all |
| `document.layers.lock.pixels` | Lock image pixels |
| `document.layers.lock.position` | Lock position |
| `document.layers.lock.transparency` | Lock transparent pixels |
| `document.layers.opacity` | Opacity |
| `document.layers.row.0` | Layer 2, Pixel / Layer 2, Smart Object, masked |
| `document.layers.row.0.disclosure` | Expand or collapse Layer 2, Smart Object, masked |
| `document.layers.row.0.maskLink` | Linked |
| `document.layers.row.0.visibility` | Hide |
| `document.layers.row.1` | Group 1, Group / Smart filter Gaussian Blur |
| `document.layers.row.1.disclosure` | Expand or collapse Group 1, Group |
| `document.layers.row.1.visibility` | Hide |
| `document.layers.row.2` | Effect Drop Shadow / Layer 1, Pixel |
| `document.layers.row.2.visibility` | Hide |
| `document.layers.row.3` | Group 1, Group |
| `document.layers.row.3.disclosure` | Expand or collapse Group 1, Group |
| `document.layers.row.3.visibility` | Hide |
| `document.layers.row.4` | Layer 1, Pixel |
| `document.layers.row.4.visibility` | Hide |
| `document.layers.smartFilter.3.0.blending` | Blending options |
| `document.layers.smartFilter.3.0.visibility` | Disable |
| `document.option.angle` | Angle |
| `document.option.antialias` | Anti-alias |
| `document.option.blendMode` | Brush blend mode |
| `document.option.brushPresets` | Brush presets |
| `document.option.combine` | Combine |
| `document.option.contiguous` | Contiguous |
| `document.option.eyedropperRadius` | Sample size |
| `document.option.feather` | Feather |
| `document.option.fillColor` | Color |
| `document.option.fillEnabled` | Fill |
| `document.option.flow` | Flow |
| `document.option.h` | H |
| `document.option.hSkew` | H skew |
| `document.option.hardness` | Hardness |
| `document.option.moveVectorMask` | Move vector mask with shape |
| `document.option.opacity` | Opacity |
| `document.option.pressureOpacity` | Pressure controls opacity |
| `document.option.pressureSize` | Pressure controls size |
| `document.option.radius` | Radius |
| `document.option.sampleAllLayers` | Sample All Layers |
| `document.option.selectAndMask` | Select and Mask… |
| `document.option.selectSubject` | Select Subject |
| `document.option.selectionMode.0` | New Selection |
| `document.option.selectionMode.1` | Add to Selection (⇧) |
| `document.option.selectionMode.2` | Subtract from Selection (⌥) |
| `document.option.selectionMode.3` | Intersect with Selection (⇧⌥) |
| `document.option.sides` | Sides |
| `document.option.size` | Size |
| `document.option.smoothing` | Smoothing |
| `document.option.star` | Star |
| `document.option.strokeColor` | Color |
| `document.option.strokeEnabled` | Stroke / Stroke colour |
| `document.option.symmetry` | Paint symmetry |
| `document.option.textAlignment.0` | Left |
| `document.option.textAlignment.1` | Center |
| `document.option.textAlignment.2` | Right |
| `document.option.textAlignment.3` | Justify |
| `document.option.textFamily` | Font for new text |
| `document.option.textSize` | Size |
| `document.option.tolerance` | Tolerance |
| `document.option.vSkew` | V skew |
| `document.option.w` | W |
| `document.option.weight` | Weight |
| `document.option.width` | Width |
| `document.option.x` | X |
| `document.option.y` | Y |
| `document.option.zoomActual` | 100 % |
| `document.option.zoomFit` | Fit on Screen |
| `document.properties.auto.analyze` | Analyze Again |
| `document.properties.auto.highlightClip` | Highlights Clip |
| `document.properties.auto.mode.0` | Tone |
| `document.properties.auto.mode.1` | Contrast |
| `document.properties.auto.mode.2` | Color |
| `document.properties.auto.shadowClip` | Shadows Clip |
| `document.properties.blackWhite.auto` | Auto |
| `document.properties.blackWhite.blues` | Blues |
| `document.properties.blackWhite.cyans` | Cyans |
| `document.properties.blackWhite.default` | Default |
| `document.properties.blackWhite.greens` | Greens |
| `document.properties.blackWhite.magentas` | Magentas |
| `document.properties.blackWhite.reds` | Reds |
| `document.properties.blackWhite.tint` | Tint |
| `document.properties.blackWhite.yellows` | Yellows |
| `document.properties.brightnessContrast.brightness` | Brightness |
| `document.properties.brightnessContrast.contrast` | Contrast |
| `document.properties.brightnessContrast.legacy` | Use Legacy |
| `document.properties.channel.0` | RGB |
| `document.properties.channel.1` | Red |
| `document.properties.channel.2` | Green |
| `document.properties.channel.3` | Blue |
| `document.properties.channelMixer.blue` | Blue |
| `document.properties.channelMixer.constant` | Constant |
| `document.properties.channelMixer.green` | Green |
| `document.properties.channelMixer.monochrome` | Monochrome |
| `document.properties.channelMixer.output.0` | Red |
| `document.properties.channelMixer.output.1` | Green |
| `document.properties.channelMixer.output.2` | Blue |
| `document.properties.channelMixer.red` | Red |
| `document.properties.colorBalance.blue` | Yellow – Blue |
| `document.properties.colorBalance.green` | Magenta – Green |
| `document.properties.colorBalance.preserveLuminosity` | Preserve Luminosity |
| `document.properties.colorBalance.red` | Cyan – Red |
| `document.properties.colorBalance.tone.0` | Shadows |
| `document.properties.colorBalance.tone.1` | Midtones |
| `document.properties.colorBalance.tone.2` | Highlights |
| `document.properties.colorLookup.dither` | Dither |
| `document.properties.colorLookup.load` | Load 3D LUT… |
| `document.properties.colorLookup.reset` | Reset |
| `document.properties.curves.reset` | Reset RGB |
| `document.properties.editContents` | Edit Contents |
| `document.properties.equalize.analyze` | Analyze Again |
| `document.properties.exposure.exposure` | Exposure |
| `document.properties.exposure.gamma` | Gamma correction |
| `document.properties.exposure.offset` | Offset |
| `document.properties.fill.addStop` | Add Stop |
| `document.properties.fill.color` | Color |
| `document.properties.fill.gradientKind.0` | Linear |
| `document.properties.fill.gradientKind.1` | Radial |
| `document.properties.fill.reverse` | Reverse |
| `document.properties.fill.stop.0.color` | Color |
| `document.properties.fill.stop.0.position` | Stop 1 |
| `document.properties.fill.stop.0.remove` | Remove this stop |
| `document.properties.fill.stop.1.color` | Color |
| `document.properties.fill.stop.1.position` | Stop 2 |
| `document.properties.fill.stop.1.remove` | Remove this stop |
| `document.properties.gradientMap.addStop` | Add Stop |
| `document.properties.gradientMap.dither` | Dither |
| `document.properties.gradientMap.method` | Perceptual |
| `document.properties.gradientMap.reverse` | Reverse |
| `document.properties.gradientMap.stop.0.color` | Color |
| `document.properties.gradientMap.stop.0.position` | Stop 1 |
| `document.properties.gradientMap.stop.0.remove` | Remove this stop |
| `document.properties.gradientMap.stop.1.color` | Color |
| `document.properties.gradientMap.stop.1.position` | Stop 2 |
| `document.properties.gradientMap.stop.1.remove` | Remove this stop |
| `document.properties.groupMode.0` | Pass Through |
| `document.properties.groupMode.1` | Isolated |
| `document.properties.hdrToning.detail` | Detail |
| `document.properties.hdrToning.exposure` | Exposure |
| `document.properties.hdrToning.gamma` | Gamma |
| `document.properties.hdrToning.highlights` | Highlight |
| `document.properties.hdrToning.method` | Local Adaptation |
| `document.properties.hdrToning.radius` | Radius |
| `document.properties.hdrToning.saturation` | Saturation |
| `document.properties.hdrToning.shadows` | Shadow |
| `document.properties.hdrToning.strength` | Strength |
| `document.properties.hdrToning.vibrance` | Vibrance |
| `document.properties.hueSaturation.colorize` | Colorize |
| `document.properties.hueSaturation.hue` | Hue |
| `document.properties.hueSaturation.lightness` | Lightness |
| `document.properties.hueSaturation.saturation` | Saturation |
| `document.properties.levels.gamma` | Gamma |
| `document.properties.levels.inBlack` | Input black |
| `document.properties.levels.inWhite` | Input white |
| `document.properties.levels.outBlack` | Output black |
| `document.properties.levels.outWhite` | Output white |
| `document.properties.matchColor.colorIntensity` | Color Intensity |
| `document.properties.matchColor.fade` | Fade |
| `document.properties.matchColor.luminance` | Luminance |
| `document.properties.matchColor.neutralize` | Neutralize |
| `document.properties.matchColor.source` | Layer 1 |
| `document.properties.name` | Layer name |
| `document.properties.photoFilter.color` | Color |
| `document.properties.photoFilter.density` | Density |
| `document.properties.photoFilter.preserveLuminosity` | Preserve Luminosity |
| `document.properties.photoFilter.preset` | Warming Filter (85) |
| `document.properties.posterize.levels` | Levels |
| `document.properties.replaceColor.color` | Color |
| `document.properties.replaceColor.fuzziness` | Fuzziness |
| `document.properties.replaceColor.hue` | Hue |
| `document.properties.replaceColor.lightness` | Lightness |
| `document.properties.replaceColor.saturation` | Saturation |
| `document.properties.replaceColor.useForeground` | Use Foreground |
| `document.properties.selectiveColor.black` | Black |
| `document.properties.selectiveColor.colors` | Reds |
| `document.properties.selectiveColor.cyan` | Cyan |
| `document.properties.selectiveColor.magenta` | Magenta |
| `document.properties.selectiveColor.method.0` | Relative |
| `document.properties.selectiveColor.method.1` | Absolute |
| `document.properties.selectiveColor.yellow` | Yellow |
| `document.properties.shadowsHighlights.blackClip` | Black Clip |
| `document.properties.shadowsHighlights.color` | Color |
| `document.properties.shadowsHighlights.highlightsAmount` | Amount |
| `document.properties.shadowsHighlights.highlightsRadius` | Radius |
| `document.properties.shadowsHighlights.highlightsTone` | Tone |
| `document.properties.shadowsHighlights.midtone` | Midtone |
| `document.properties.shadowsHighlights.shadowsAmount` | Amount |
| `document.properties.shadowsHighlights.shadowsRadius` | Radius |
| `document.properties.shadowsHighlights.shadowsTone` | Tone |
| `document.properties.shadowsHighlights.whiteClip` | White Clip |
| `document.properties.style.0` | Drop Shadow, 5 px · 120° |
| `document.properties.style.edit` | Edit… |
| `document.properties.threshold.level` | Threshold level |
| `document.properties.vibrance.saturation` | Saturation |
| `document.properties.vibrance.vibrance` | Vibrance |
| `document.remove.backend.0` | Auto |
| `document.remove.backend.1` | PatchMatch |
| `document.remove.backend.2` | LaMa |
| `document.remove.dilation` | Expand |
| `document.remove.distractions` | Remove Distractions… |
| `document.remove.selection` | Remove Selection |
| `document.remove.size` | Size |
| `document.shape.convert` | Convert to Pixels |
| `document.shape.fill.color` | Color |
| `document.shape.fill.kind.0` | None |
| `document.shape.fill.kind.1` | Solid |
| `document.shape.fill.kind.2` | Gradient |
| `document.shape.mask.add` | Add Vector Mask |
| `document.shape.mask.fromSelection` | From Selection |
| `document.shape.rect.height` | Height |
| `document.shape.rect.linkRadii` | Same radius for all corners |
| `document.shape.rect.radius` | Corner radius |
| `document.shape.rect.width` | Width |
| `document.shape.stroke.alignment.0` | Inside |
| `document.shape.stroke.alignment.1` | Center |
| `document.shape.stroke.alignment.2` | Outside |
| `document.shape.stroke.cap.0` | Butt |
| `document.shape.stroke.cap.1` | Square |
| `document.shape.stroke.cap.2` | Round |
| `document.shape.stroke.color` | Color |
| `document.shape.stroke.dashes` | Stroke dashes |
| `document.shape.stroke.join` | Miter |
| `document.shape.stroke.kind.0` | None |
| `document.shape.stroke.kind.1` | Solid |
| `document.shape.stroke.kind.2` | Gradient |
| `document.shape.stroke.miter` | Miter limit |
| `document.shape.stroke.width` | Width |
| `document.tabs.0.close` | Close Untitled |
| `document.tabs.new` | New Document |
| `document.text.alignment.0` | Left |
| `document.text.alignment.1` | Center |
| `document.text.alignment.2` | Right |
| `document.text.alignment.3` | Justify |
| `document.text.apply` | Apply |
| `document.text.baselineShift` | Baseline shift |
| `document.text.cancel` | Cancel |
| `document.text.color` | Color |
| `document.text.convert` | Convert to Pixels |
| `document.text.family` | Font family |
| `document.text.firstLineIndent` | First line indent |
| `document.text.kerning` | Kerning |
| `document.text.leading` | Leading |
| `document.text.leftIndent` | Left indent |
| `document.text.optionsApply` | Apply |
| `document.text.optionsCancel` | Cancel |
| `document.text.rightIndent` | Right indent |
| `document.text.size` | Size |
| `document.text.spaceAfter` | Space after |
| `document.text.spaceBefore` | Space before |
| `document.text.style` | Font style |
| `document.text.toggleBox` | Convert to Paragraph Text |
| `document.text.tracking` | Tracking |
| `document.tool.brush` | Brush (B) |
| `document.tool.cloneStamp` | Clone Stamp (S) |
| `document.tool.contentAwareMove` | Content-Aware Move |
| `document.tool.crop` | Crop (C) |
| `document.tool.directSelect` | Direct Selection (A) |
| `document.tool.ellipseMarquee` | Elliptical Marquee (M) |
| `document.tool.ellipseShape` | Ellipse (U) |
| `document.tool.eraser` | Eraser (E) |
| `document.tool.eyedropper` | Eyedropper (I) |
| `document.tool.gradient` | Gradient (G) |
| `document.tool.hand` | Hand (H) |
| `document.tool.heal` | Healing Brush (J) |
| `document.tool.lasso` | Lasso (L) |
| `document.tool.lineShape` | Line (U) |
| `document.tool.magneticLasso` | Magnetic Lasso (L) |
| `document.tool.marquee` | Rectangular Marquee (M) |
| `document.tool.move` | Move (V) |
| `document.tool.objectSelect` | Object Selection (W) |
| `document.tool.pathSelect` | Path Selection (A) |
| `document.tool.pen` | Pen (P) |
| `document.tool.polygonLasso` | Polygonal Lasso (L) |
| `document.tool.polygonShape` | Polygon (U) |
| `document.tool.quickSelect` | Quick Selection (W) |
| `document.tool.rectangleShape` | Rectangle (U) |
| `document.tool.remove` | Remove (⇧J) |
| `document.tool.type` | Type (T) |
| `document.tool.wand` | Magic Wand (W) |
| `document.tool.zoom` | Zoom (Z) |
| `document.toolbar.inspector` | Inspector |
| `document.toolbar.library` | Library |
| `document.toolbar.open` | Open Folder… |
| `document.toolbar.sidebar` | Hide Sidebar |
| `document.transform.amount` | Amount |
| `document.transform.apply` | Apply… |
| `document.transform.bend` | Bend |
| `document.transform.cancel` | Cancel |
| `document.transform.commit` | Commit |
| `document.transform.expansion` | Expansion |
| `document.transform.grid` | Warp grid |
| `document.transform.height` | H |
| `document.transform.interpolation` | Interpolation |
| `document.transform.perspectiveMode.0` | Layout |
| `document.transform.perspectiveMode.1` | Warp |
| `document.transform.protectChannel` | Protect channel |
| `document.transform.puppetDensity.0` | Sparse |
| `document.transform.puppetDensity.1` | Normal |
| `document.transform.puppetDensity.2` | Dense |
| `document.transform.puppetMode` | Normal |
| `document.transform.reset` | Reset |
| `document.transform.rotate` | Rotate |
| `document.transform.showMesh` | Show Mesh |
| `document.transform.splitHorizontal` | Split Horizontally |
| `document.transform.splitVertical` | Split Vertically |
| `document.transform.warpPreset` | Custom |
| `document.transform.warpSplit.0` | Drag points and handles |
| `document.transform.warpSplit.1` | Split vertically: click the net |
| `document.transform.warpSplit.2` | Split horizontally: click the net |
| `document.transform.warpSplit.3` | Split crosswise: click the net (⇧ keeps splitting) |
| `document.transform.width` | W |

## B5-50b shared shell toolbar compatibility

`ContentView.axMode` applies only to newly identified Library/Develop controls.
Document mode retains these established identifiers (also listed above):

| Identifier | Control |
| --- | --- |
| `document.toolbar.open` | Open Folder |
| `document.toolbar.inspector` | Inspector visibility |
| `document.toolbar.sidebar` | Document sidebar visibility |
| `document.toolbar.library` | Return to Library |

Library/Develop use the native SwiftUI sidebar toggle. Existing shared actions retain
`toolbar-assist`, `toolbar-assist-menu`, `toolbar-people-merge`, `toolbar-auto-edit`,
and `toolbar-agent-review`; they do not acquire document-prefixed aliases.
The full restoration inventory is in [B5-50](../B5-50/RESTORED-IDENTIFIERS.md).
