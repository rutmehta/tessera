# Tessera keyboard shortcuts

Generated from SwiftUI Commands, KeyRouter and its delegated key maps. Regenerate with
`python3 tools/orchestrate/shortcut-audit.py --write-doc`; the Swift gate checks this file.

## Menu key equivalents

Modes describe registered equivalents, including disabled items and diagnostic commands.
A disabled command may be unavailable for the current selection. Conditional `.shortcut`
bindings are absent outside their mode. System-provided Quit, Hide, Minimize, Settings,
window cycling and other standard macOS commands retain their native meanings; custom
commands may not claim the reserved combinations listed below.

| Mode | Shortcut | Command |
| --- | --- | --- |
| Document / Library | ⌘N | AppCommands / New Document… |
| Document / Library | ⇧⌘O | AppCommands / Open Document… |
| Document / Library | ⌘O | AppCommands / Open Folder… |
| Library | ⇧⌘I | AppCommands / Import Lightroom Catalog… |
| Document / Library | ⇧⌘T | AppCommands / Capture |
| Library | ⇧⌘E | AppCommands / Export… |
| Document / Library | ⇧⌘P | AppCommands / Page Setup… |
| Document / Library | ⌘P | AppCommands / Print… |
| Document / Library | ⌘Z | AppCommands / Undo (dynamic title) |
| Document / Library | ⇧⌘Z | AppCommands / Redo (dynamic title) |
| Document / Library | ⌘A | AppCommands / Select All / Select All Images |
| Document | ⌘= | AppCommands / Zoom In |
| Document | ⌘- | AppCommands / Zoom Out |
| Document | ⌘0 | AppCommands / Fit on Screen |
| Document | ⌘1 | AppCommands / 100 % |
| Document / Library | ⌥⌘I | AppCommands / Hide Inspector / Show Inspector |
| Document / Library | ⌥⌘F | AppCommands / Hide Filmstrip / Show Filmstrip |
| Document / Library | ⇧⌘D | AppCommands / Defect Sweep… |
| Document / Library | ⌥⌘A | AppCommands / Assist |
| Document / Library | ⌘Delete | AppCommands / Delete from Disk… |
| Library | ⌘E | AppCommands / Open in Layers… |
| Document / Library | ⌥⌘N | AppCommands / New Album… |
| Document / Library | ⌥⇧⌘N | AppCommands / New Smart Album… |
| Document / Library | ⌥⌘L | AppCommands / Clear Filter |
| Document / Library | ⌥⌘P | AppCommands / Show People |
| Document / Library | ⌥⌘K | AppCommands / Suggest Keywords for Selection |
| Library | ⇧⌘A | AppCommands / Auto Edit… |
| Document / Library | ⇧⌘R | AppCommands / Reset All Settings |
| Library | ⇧⌘S | AppCommands / New Snapshot… |
| Document / Library | ⌥⌘T | AppCommands / Show Render Timing |
| Library | ⇧⌘N | AppCommands / Load 20,000 Stub Items |
| Library | ⇧⌘B | AppCommands / Run Grid Scroll Benchmark |
| Document / Library | ⌘W | AppCommands / Close Document / Close |
| Document / Library | ⌘S | AppCommands / Save |
| Document | ⇧⌘S | AppCommands / Save As… |
| Document | ⇧⌘E | AppCommands / Export Flat… |
| Document / Library | ⌃H | PhotoMenuItems / HDR… |
| Document / Library | ⌃M | PhotoMenuItems / Panorama… |
| Document / Library | ⌃⌥I | PhotoMenuItems / Enhance… |
| Document | ⇧⌘N | LayerMenu / Layer |
| Document / Library | ⌘J | LayerMenu / Duplicate Layer |
| Document / Library | ⌘G | LayerMenu / Group Layers |
| Document / Library | ⇧⌘G | LayerMenu / Ungroup Layers |
| Document / Library | ⌥⌘G | LayerMenu / Release Clipping Mask / Create Clipping Mask |
| Document | ⌘E | LayerMenu / Merge Down |
| Document / Library | ⌘L | ImageMenu / levels |
| Document / Library | ⌘U | ImageMenu / hueSaturation |
| Document / Library | ⌘B | ImageMenu / colorBalance |
| Document / Library | ⌥⇧⌘B | ImageMenu / blackWhite |
| Document / Library | ⌘I | ImageMenu / invert |
| Document / Library | ⇧⌘U | ImageMenu / desaturate |
| Document / Library | ⇧⌘L | ImageMenu / tone |
| Document / Library | ⌥⇧⌘L | ImageMenu / contrast |
| Document | ⇧⌘B | ImageMenu / color |
| Document / Library | ⌃F | FilterMenu / Last Filter (dynamic title) |
| Document / Library | ⇧⌘X | LiquifyMenuItem / Liquify… |
| Document | ⇧⌘A | CameraRawMenuItem / Camera Raw Filter… |
| Document | ⌥⇧⌘A | AdaptiveWideAngleMenuItem / Adaptive Wide Angle… |
| Document | ⌘D | SelectMenuItems / Deselect |
| Document | ⇧⌘I | SelectMenuItems / Inverse |
| Document | ⌥⌘R | SelectMenuItems / Select and Mask… |
| Document | ⌘T | EditToolsMenuItems / Free Transform |
| Document | ⌥⇧⌘C | AdvancedTransformMenuItems / Content-Aware Scale |

## Reserved for macOS

F11, F12, ⇧⌘3, ⇧⌘4, ⇧⌘5, ⌃F2, ⌃F3, ⌃⌘Q, ⌘,, ⌘H, ⌘M, ⌘Q, ⌘Space, ⌘Tab, ⌘`, ⌥⌘H

## Routed keys and context

Library culling, Compare, Review and Photo Edit have separate routing contexts.
Document tools run before the fallback document map. Tool groups intentionally share
a letter; Shift cycles that group (A also cycles on repetition). Shift-J activates Remove
before the Healing tool map. Return/Escape/Delete act on the active edit, selection or
tool session before falling back. Command-E in Library is a router alias of Open in Layers.
Command entries in DocumentKeyMap describe menu actions; KeyRouter passes these to menus.
Text fields, key-owning controls, panels and sheets have the priority shown in the guards.

The following source-generated reference includes every routed key, its action, modifiers
and precedence, including consumed keys that deliberately do nothing in Review/Photo Edit.
Keeping the guards here avoids documenting context-dependent keys as global shortcuts.

| Context | Key | Action / tool group |
| --- | --- | --- |
| Library/Compare override | Return | model.chooseInCompare(); return true |
| Library/Compare override | Enter | model.chooseInCompare(); return true |
| Library/Compare override | Escape | model.exitCompare(); return true |
| Library/Cull | Delete | model.deletePressed(); return true |
| Library/Cull | ForwardDelete | model.deletePressed(); return true |
| Library/Cull | Left | model.navigate(.left, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | Right | model.navigate(.right, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | Up | model.navigate(.up, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | Down | model.navigate(.down, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | Return | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | Enter | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | Escape | if loupe { model.requestViewMode(.grid); return true }; return false |
| Library/Cull | X | model.perform(.reject) |
| Library/Cull | U | model.perform(.undecided) |
| Library/Cull | P | model.perform(.keep) |
| Library/Cull | 1 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | 2 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | 3 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | 6 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | 7 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | 8 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | 9 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | B | model.perform(.toggleBasket) |
| Library/Cull | A | model.autoAdvance.toggle(); model.statusMessage = "Auto-advance \(model.autoAdvance ? "on" : "off")" |
| Library/Cull | K | model.keepBestRejectRest() |
| Library/Cull | Y | model.assist.confirmAll() |
| Library/Cull | N | model.assist.dismiss(model.targetIDs) |
| Library/Cull | C | comparing ? model.exitCompare() : model.enterCompare() |
| Library/Compare override | Z | model.toggleCompareZoom() |
| Library/Loupe | S | context-dependent action (see routing reference) |
| Library/Cull | D | model.enterPhotoEdit() |
| Library/Cull | G | model.requestViewMode(.grid) |
| Library/Cull | E | model.requestViewMode(.loupe) |
| Library/Review | Escape | model.backFromReview(); return true |
| Library/Review | Left | model.moveReviewSelection(-1); return true |
| Library/Review | Up | model.moveReviewSelection(-1); return true |
| Library/Review | Right | model.moveReviewSelection(1); return true |
| Library/Review | Down | model.moveReviewSelection(1); return true |
| Library/Review | D | model.editReviewedPhoto() |
| Library/Review | E | model.editReviewedPhoto() |
| Library/Review | G | model.returnToLibrary(grid: true) |
| Library/Review | X | break |
| Library/Review | U | break |
| Library/Review | P | break |
| Library/Review | 1 | break |
| Library/Review | 2 | break |
| Library/Review | 3 | break |
| Library/Review | 6 | break |
| Library/Review | 7 | break |
| Library/Review | 8 | break |
| Library/Review | 9 | break |
| Library/Review | B | break |
| Library/Review | A | break |
| Library/Review | K | break |
| Library/Review | Y | break |
| Library/Review | N | break |
| Library/Review | C | break |
| Library/Photo Edit | Escape | model.returnFromPhotoEdit(); return true |
| Library/Photo Edit | Left | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | Up | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | Right | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | Down | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | D | model.photoInspectorTab = .develop |
| Library/Photo Edit | M | model.photoInspectorTab = .masks |
| Library/Photo Edit | G | model.returnToLibrary(grid: true) |
| Library/Photo Edit | S | context-dependent action (see routing reference) |
| Library/Photo Edit | X | break |
| Library/Photo Edit | U | break |
| Library/Photo Edit | P | break |
| Library/Photo Edit | 1 | break |
| Library/Photo Edit | 2 | break |
| Library/Photo Edit | 3 | break |
| Library/Photo Edit | 6 | break |
| Library/Photo Edit | 7 | break |
| Library/Photo Edit | 8 | break |
| Library/Photo Edit | 9 | break |
| Library/Photo Edit | B | break |
| Library/Photo Edit | A | break |
| Library/Photo Edit | K | break |
| Library/Photo Edit | Y | break |
| Library/Photo Edit | N | break |
| Library/Photo Edit | C | break |
| Document/Tools | V | .move |
| Document/Tools | M | .marquee, .ellipseMarquee |
| Document/Tools | L | .lasso, .polygonLasso, .magneticLasso |
| Document/Tools | W | .quickSelect, .wand, .objectSelect |
| Document/Tools | B | .brush |
| Document/Tools | E | .eraser |
| Document/Tools | S | .cloneStamp |
| Document/Tools | J | .heal |
| Document/Tools | G | .gradient |
| Document/Tools | C | .crop |
| Document/Tools | T | .type |
| Document/Tools | I | .eyedropper |
| Document/Tools | H | .hand |
| Document/Tools | Z | .zoom |
| Document/Tools | U | .rectangleShape, .ellipseShape, .polygonShape, .lineShape |
| Document/Tools | P | .pen |
| Document/Tools | A | .pathSelect, .directSelect |
| Library/Compare override | ⇧Return | model.chooseInCompare(); return true |
| Library/Compare override | ⌥Return | model.chooseInCompare(); return true |
| Library/Compare override | ⌥⇧Return | model.chooseInCompare(); return true |
| Library/Compare override | ⇧Enter | model.chooseInCompare(); return true |
| Library/Compare override | ⌥Enter | model.chooseInCompare(); return true |
| Library/Compare override | ⌥⇧Enter | model.chooseInCompare(); return true |
| Library/Compare override | ⇧Escape | model.exitCompare(); return true |
| Library/Compare override | ⌥Escape | model.exitCompare(); return true |
| Library/Compare override | ⌥⇧Escape | model.exitCompare(); return true |
| Library/Cull | ⇧Delete | model.deletePressed(); return true |
| Library/Cull | ⌥Delete | model.deletePressed(); return true |
| Library/Cull | ⌥⇧Delete | model.deletePressed(); return true |
| Library/Cull | ⇧ForwardDelete | model.deletePressed(); return true |
| Library/Cull | ⌥ForwardDelete | model.deletePressed(); return true |
| Library/Cull | ⌥⇧ForwardDelete | model.deletePressed(); return true |
| Library/Cull | ⇧Left | model.navigate(.left, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥Left | model.navigate(.left, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥⇧Left | model.navigate(.left, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⇧Right | model.navigate(.right, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥Right | model.navigate(.right, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥⇧Right | model.navigate(.right, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⇧Up | model.navigate(.up, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥Up | model.navigate(.up, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥⇧Up | model.navigate(.up, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⇧Down | model.navigate(.down, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥Down | model.navigate(.down, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⌥⇧Down | model.navigate(.down, groupwise: loupe &#124;&#124; option, extend: shift); return true |
| Library/Cull | ⇧Return | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | ⌥Return | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | ⌥⇧Return | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | ⇧Enter | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | ⌥Enter | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | ⌥⇧Enter | model.requestViewMode(loupe ? .grid : .loupe); return true |
| Library/Cull | ⇧Escape | if loupe { model.requestViewMode(.grid); return true }; return false |
| Library/Cull | ⌥Escape | if loupe { model.requestViewMode(.grid); return true }; return false |
| Library/Cull | ⌥⇧Escape | if loupe { model.requestViewMode(.grid); return true }; return false |
| Library/Cull | ⇧X | model.perform(.reject) |
| Library/Cull | ⌥X | model.perform(.reject) |
| Library/Cull | ⌥⇧X | model.perform(.reject) |
| Library/Cull | ⇧U | model.perform(.undecided) |
| Library/Cull | ⌥U | model.perform(.undecided) |
| Library/Cull | ⌥⇧U | model.perform(.undecided) |
| Library/Cull | ⇧P | model.perform(.keep) |
| Library/Cull | ⌥P | model.perform(.keep) |
| Library/Cull | ⌥⇧P | model.perform(.keep) |
| Library/Cull | ⇧1 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⌥1 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⌥⇧1 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⇧2 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⌥2 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⌥⇧2 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⇧3 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⌥3 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⌥⇧3 | model.perform(.grade(UInt8(ch)!)) |
| Library/Cull | ⇧6 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥6 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥⇧6 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⇧7 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥7 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥⇧7 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⇧8 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥8 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥⇧8 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⇧9 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥9 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⌥⇧9 | model.perform(.mark(UInt8(ch)!)) |
| Library/Cull | ⇧B | model.perform(.toggleBasket) |
| Library/Cull | ⌥B | model.perform(.toggleBasket) |
| Library/Cull | ⌥⇧B | model.perform(.toggleBasket) |
| Library/Cull | ⇧A | model.autoAdvance.toggle(); model.statusMessage = "Auto-advance \(model.autoAdvance ? "on" : "off")" |
| Library/Cull | ⌥A | model.autoAdvance.toggle(); model.statusMessage = "Auto-advance \(model.autoAdvance ? "on" : "off")" |
| Library/Cull | ⌥⇧A | model.autoAdvance.toggle(); model.statusMessage = "Auto-advance \(model.autoAdvance ? "on" : "off")" |
| Library/Cull | ⇧K | model.keepBestRejectRest() |
| Library/Cull | ⌥K | model.keepBestRejectRest() |
| Library/Cull | ⌥⇧K | model.keepBestRejectRest() |
| Library/Cull | ⇧Y | model.assist.confirmAll() |
| Library/Cull | ⌥Y | model.assist.confirmAll() |
| Library/Cull | ⌥⇧Y | model.assist.confirmAll() |
| Library/Cull | ⇧N | model.assist.dismiss(model.targetIDs) |
| Library/Cull | ⌥N | model.assist.dismiss(model.targetIDs) |
| Library/Cull | ⌥⇧N | model.assist.dismiss(model.targetIDs) |
| Library/Cull | ⇧C | comparing ? model.exitCompare() : model.enterCompare() |
| Library/Cull | ⌥C | comparing ? model.exitCompare() : model.enterCompare() |
| Library/Cull | ⌥⇧C | comparing ? model.exitCompare() : model.enterCompare() |
| Library/Compare override | ⇧Z | model.toggleCompareZoom() |
| Library/Compare override | ⌥Z | model.toggleCompareZoom() |
| Library/Compare override | ⌥⇧Z | model.toggleCompareZoom() |
| Library/Loupe | ⇧S | context-dependent action (see routing reference) |
| Library/Loupe | ⌥S | context-dependent action (see routing reference) |
| Library/Loupe | ⌥⇧S | context-dependent action (see routing reference) |
| Library/Cull | ⇧D | model.enterPhotoEdit() |
| Library/Cull | ⌥D | model.enterPhotoEdit() |
| Library/Cull | ⌥⇧D | model.enterPhotoEdit() |
| Library/Cull | ⇧G | model.requestViewMode(.grid) |
| Library/Cull | ⌥G | model.requestViewMode(.grid) |
| Library/Cull | ⌥⇧G | model.requestViewMode(.grid) |
| Library/Cull | ⇧E | model.requestViewMode(.loupe) |
| Library/Cull | ⌥E | model.requestViewMode(.loupe) |
| Library/Cull | ⌥⇧E | model.requestViewMode(.loupe) |
| Library/Review | ⇧Escape | model.backFromReview(); return true |
| Library/Review | ⌥Escape | model.backFromReview(); return true |
| Library/Review | ⌥⇧Escape | model.backFromReview(); return true |
| Library/Review | ⇧Left | model.moveReviewSelection(-1); return true |
| Library/Review | ⌥Left | model.moveReviewSelection(-1); return true |
| Library/Review | ⌥⇧Left | model.moveReviewSelection(-1); return true |
| Library/Review | ⇧Up | model.moveReviewSelection(-1); return true |
| Library/Review | ⌥Up | model.moveReviewSelection(-1); return true |
| Library/Review | ⌥⇧Up | model.moveReviewSelection(-1); return true |
| Library/Review | ⇧Right | model.moveReviewSelection(1); return true |
| Library/Review | ⌥Right | model.moveReviewSelection(1); return true |
| Library/Review | ⌥⇧Right | model.moveReviewSelection(1); return true |
| Library/Review | ⇧Down | model.moveReviewSelection(1); return true |
| Library/Review | ⌥Down | model.moveReviewSelection(1); return true |
| Library/Review | ⌥⇧Down | model.moveReviewSelection(1); return true |
| Library/Review | ⇧D | model.editReviewedPhoto() |
| Library/Review | ⌥D | model.editReviewedPhoto() |
| Library/Review | ⌥⇧D | model.editReviewedPhoto() |
| Library/Review | ⇧E | model.editReviewedPhoto() |
| Library/Review | ⌥E | model.editReviewedPhoto() |
| Library/Review | ⌥⇧E | model.editReviewedPhoto() |
| Library/Review | ⇧G | model.returnToLibrary(grid: true) |
| Library/Review | ⌥G | model.returnToLibrary(grid: true) |
| Library/Review | ⌥⇧G | model.returnToLibrary(grid: true) |
| Library/Review | ⇧X | break |
| Library/Review | ⌥X | break |
| Library/Review | ⌥⇧X | break |
| Library/Review | ⇧U | break |
| Library/Review | ⌥U | break |
| Library/Review | ⌥⇧U | break |
| Library/Review | ⇧P | break |
| Library/Review | ⌥P | break |
| Library/Review | ⌥⇧P | break |
| Library/Review | ⇧1 | break |
| Library/Review | ⌥1 | break |
| Library/Review | ⌥⇧1 | break |
| Library/Review | ⇧2 | break |
| Library/Review | ⌥2 | break |
| Library/Review | ⌥⇧2 | break |
| Library/Review | ⇧3 | break |
| Library/Review | ⌥3 | break |
| Library/Review | ⌥⇧3 | break |
| Library/Review | ⇧6 | break |
| Library/Review | ⌥6 | break |
| Library/Review | ⌥⇧6 | break |
| Library/Review | ⇧7 | break |
| Library/Review | ⌥7 | break |
| Library/Review | ⌥⇧7 | break |
| Library/Review | ⇧8 | break |
| Library/Review | ⌥8 | break |
| Library/Review | ⌥⇧8 | break |
| Library/Review | ⇧9 | break |
| Library/Review | ⌥9 | break |
| Library/Review | ⌥⇧9 | break |
| Library/Review | ⇧B | break |
| Library/Review | ⌥B | break |
| Library/Review | ⌥⇧B | break |
| Library/Review | ⇧A | break |
| Library/Review | ⌥A | break |
| Library/Review | ⌥⇧A | break |
| Library/Review | ⇧K | break |
| Library/Review | ⌥K | break |
| Library/Review | ⌥⇧K | break |
| Library/Review | ⇧Y | break |
| Library/Review | ⌥Y | break |
| Library/Review | ⌥⇧Y | break |
| Library/Review | ⇧N | break |
| Library/Review | ⌥N | break |
| Library/Review | ⌥⇧N | break |
| Library/Review | ⇧C | break |
| Library/Review | ⌥C | break |
| Library/Review | ⌥⇧C | break |
| Library/Photo Edit | ⇧Escape | model.returnFromPhotoEdit(); return true |
| Library/Photo Edit | ⌥Escape | model.returnFromPhotoEdit(); return true |
| Library/Photo Edit | ⌥⇧Escape | model.returnFromPhotoEdit(); return true |
| Library/Photo Edit | ⇧Left | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥Left | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥⇧Left | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⇧Up | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥Up | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥⇧Up | model.navigate(.left, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⇧Right | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥Right | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥⇧Right | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⇧Down | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥Down | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⌥⇧Down | model.navigate(.right, groupwise: false, extend: false); return true |
| Library/Photo Edit | ⇧D | model.photoInspectorTab = .develop |
| Library/Photo Edit | ⌥D | model.photoInspectorTab = .develop |
| Library/Photo Edit | ⌥⇧D | model.photoInspectorTab = .develop |
| Library/Photo Edit | ⇧M | model.photoInspectorTab = .masks |
| Library/Photo Edit | ⌥M | model.photoInspectorTab = .masks |
| Library/Photo Edit | ⌥⇧M | model.photoInspectorTab = .masks |
| Library/Photo Edit | ⇧G | model.returnToLibrary(grid: true) |
| Library/Photo Edit | ⌥G | model.returnToLibrary(grid: true) |
| Library/Photo Edit | ⌥⇧G | model.returnToLibrary(grid: true) |
| Library/Photo Edit | ⇧S | context-dependent action (see routing reference) |
| Library/Photo Edit | ⌥S | context-dependent action (see routing reference) |
| Library/Photo Edit | ⌥⇧S | context-dependent action (see routing reference) |
| Library/Photo Edit | ⇧X | break |
| Library/Photo Edit | ⌥X | break |
| Library/Photo Edit | ⌥⇧X | break |
| Library/Photo Edit | ⇧U | break |
| Library/Photo Edit | ⌥U | break |
| Library/Photo Edit | ⌥⇧U | break |
| Library/Photo Edit | ⇧P | break |
| Library/Photo Edit | ⌥P | break |
| Library/Photo Edit | ⌥⇧P | break |
| Library/Photo Edit | ⇧1 | break |
| Library/Photo Edit | ⌥1 | break |
| Library/Photo Edit | ⌥⇧1 | break |
| Library/Photo Edit | ⇧2 | break |
| Library/Photo Edit | ⌥2 | break |
| Library/Photo Edit | ⌥⇧2 | break |
| Library/Photo Edit | ⇧3 | break |
| Library/Photo Edit | ⌥3 | break |
| Library/Photo Edit | ⌥⇧3 | break |
| Library/Photo Edit | ⇧6 | break |
| Library/Photo Edit | ⌥6 | break |
| Library/Photo Edit | ⌥⇧6 | break |
| Library/Photo Edit | ⇧7 | break |
| Library/Photo Edit | ⌥7 | break |
| Library/Photo Edit | ⌥⇧7 | break |
| Library/Photo Edit | ⇧8 | break |
| Library/Photo Edit | ⌥8 | break |
| Library/Photo Edit | ⌥⇧8 | break |
| Library/Photo Edit | ⇧9 | break |
| Library/Photo Edit | ⌥9 | break |
| Library/Photo Edit | ⌥⇧9 | break |
| Library/Photo Edit | ⇧B | break |
| Library/Photo Edit | ⌥B | break |
| Library/Photo Edit | ⌥⇧B | break |
| Library/Photo Edit | ⇧A | break |
| Library/Photo Edit | ⌥A | break |
| Library/Photo Edit | ⌥⇧A | break |
| Library/Photo Edit | ⇧K | break |
| Library/Photo Edit | ⌥K | break |
| Library/Photo Edit | ⌥⇧K | break |
| Library/Photo Edit | ⇧Y | break |
| Library/Photo Edit | ⌥Y | break |
| Library/Photo Edit | ⌥⇧Y | break |
| Library/Photo Edit | ⇧N | break |
| Library/Photo Edit | ⌥N | break |
| Library/Photo Edit | ⌥⇧N | break |
| Library/Photo Edit | ⇧C | break |
| Library/Photo Edit | ⌥C | break |
| Library/Photo Edit | ⌥⇧C | break |
| Document/Tools | ⇧V | .move |
| Document/Tools | ⇧M | .marquee, .ellipseMarquee |
| Document/Tools | ⇧L | .lasso, .polygonLasso, .magneticLasso |
| Document/Tools | ⇧W | .quickSelect, .wand, .objectSelect |
| Document/Tools | ⇧B | .brush |
| Document/Tools | ⇧E | .eraser |
| Document/Tools | ⇧S | .cloneStamp |
| Document/Tools | ⇧J | Remove tool |
| Document/Tools | ⇧G | .gradient |
| Document/Tools | ⇧C | .crop |
| Document/Tools | ⇧T | .type |
| Document/Tools | ⇧I | .eyedropper |
| Document/Tools | ⇧H | .hand |
| Document/Tools | ⇧Z | .zoom |
| Document/Tools | ⇧U | .rectangleShape, .ellipseShape, .polygonShape, .lineShape |
| Document/Tools | ⇧P | .pen |
| Document/Tools | ⇧A | .pathSelect, .directSelect |
| Document/Tools | Return | .commit |
| Document/Tools | ⇧Return | .commit |
| Document/Tools | Enter | .commit |
| Document/Tools | ⇧Enter | .commit |
| Document/Tools | Escape | .cancel |
| Document/Tools | ⇧Escape | .cancel |
| Document/Tools | [ | shift ? .brushHardness(harder: false) : .brushSize(larger: false) |
| Document/Tools | ⇧[ | shift ? .brushHardness(harder: false) : .brushSize(larger: false) |
| Document/Tools | ] | shift ? .brushHardness(harder: true) : .brushSize(larger: true) |
| Document/Tools | ⇧] | shift ? .brushHardness(harder: true) : .brushSize(larger: true) |
| Document/Tools | X | .swapColors |
| Document/Tools | D | .defaultColors |
| Document/Tools | 0 | opacityDigit |
| Document/Tools | 1 | opacityDigit |
| Document/Tools | 2 | opacityDigit |
| Document/Tools | 3 | opacityDigit |
| Document/Tools | 4 | opacityDigit |
| Document/Tools | 5 | opacityDigit |
| Document/Tools | 6 | opacityDigit |
| Document/Tools | 7 | opacityDigit |
| Document/Tools | 8 | opacityDigit |
| Document/Tools | 9 | opacityDigit |
| Document/Tools | Space | .panHold |
| Document/Tools | ⇧Space | .panHold |
| Document/Tools | ⌥Space | .panHold |
| Document/Tools | ⌥⇧Space | .panHold |
| Document/Tools | Tab | mods.isEmpty ? .togglePanels : nil |
| Document/Tools | Delete | mods.isEmpty ? .deleteLayer : nil |
| Document/Tools | ForwardDelete | mods.isEmpty ? .deleteLayer : nil |
| Document/Tools | F | .cycleScreenMode |
| Document/Tools | ⇧F | .cycleScreenMode |
| Document/Tools | Q | Quick Mask |
| Library/Menu alias | ⌘E | Open in Layers |

Library letters also accept Shift/Option; Shift-S selects gamut warning, and
Shift/Option arrows extend selection or navigate groups. The full modifier guards,
special keys, digit/bracket maps and delegated edit-session bindings follow.

### Tessera/App/KeyRouter.swift

```swift
import AppKit
import TesseraCore
@MainActor protocol KeyOwningControl: AnyObject {}
@MainActor
final class KeyRouter {
    private var monitor: Any?
    private let model: AppModel
    private let focusTrace: InspectorFocusTrace?
    init(model: AppModel) {
        self.model = model
        focusTrace = InspectorFocusTrace.configured(arguments: CommandLine.arguments)
    }
    func install() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            nonisolated(unsafe) let e = event
            let handled = MainActor.assumeIsolated {
                InspectorFocusTrace.routeEvent(self?.focusTrace, event: e,
                    document: self?.model.viewMode == .document,
                    ownedWindow: self?.model.documents.current?.viewport?.window) {
                    e.type == .keyUp ? (self?.handleKeyUp(e) ?? false) : (self?.handle(e) ?? false)
                }
            }
            return handled ? nil : event
        }
    }
    private func shouldIgnore(_ event: NSEvent) -> Bool {
        if isBusyWindow(event) { return true }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        return mods.contains(.command) || mods.contains(.control)
    }
    private func isBusyWindow(_ event: NSEvent) -> Bool {
        guard let window = event.window else { return true }
        if window is NSPanel || window.attachedSheet != nil || window.sheetParent != nil || NSApp.modalWindow != nil { return true }
        return window.firstResponder is NSText || window.firstResponder is NSTextField || window.firstResponder is KeyOwningControl
    }
    func handleEditInLayers(_ event: NSEvent) -> Bool {
        guard model.viewMode != .document, !model.isReviewing, model.source != .people, !isBusyWindow(event),
              event.modifierFlags.intersection([.command, .shift, .option, .control]) == .command,
              event.charactersIgnoringModifiers?.lowercased() == "e" else { return false }
        guard model.documents.opening == nil else { return true }
        model.requestLayeredCopy()
        return true
    }
    func handleKeyUp(_ event: NSEvent) -> Bool {
        guard event.keyCode == 49, model.documents.spaceHeld else { return false }
        model.documents.spaceHeld = false
        model.documents.current?.viewport?.cursorDidChange()
        return model.viewMode == .document
    }
    func handle(_ event: NSEvent) -> Bool {
        if model.loupeDisclosurePresented {
            guard event.keyCode == 53, let dismiss = model.dismissLoupeDisclosure else { return false }
            dismiss()
            model.loupeDisclosurePresented = false
            model.dismissLoupeDisclosure = nil
            return true
        }
        if model.viewMode == .document, DocumentText.shared.routeSessionKey(event) { return true }
        if handleEditInLayers(event) { return true }
        if model.viewMode == .document, handleToolLetterOverKeyOwner(event) { return true }
        if shouldIgnore(event) { return false }
        if model.viewMode == .document { return handleDocument(event) }
        if model.isReviewing { return handleReview(event) }
        if model.source == .people && !model.isPhotoEditing { return false }
        if model.isPhotoEditing {
            if MaskTools.shared.model === model, MaskTools.shared.handleKey(event) { return true }
            if DevelopTools.shared.model === model, DevelopTools.shared.handleKey(event) { return true }
            return handlePhotoEdit(event)
        }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let shift = mods.contains(.shift)
        let option = mods.contains(.option)
        let loupe = model.viewMode == .loupe
        let comparing = model.viewMode == .compare
        if comparing {
            switch event.keyCode {
            case 36, 76: model.chooseInCompare(); return true
            case 53: model.exitCompare(); return true
            default: break
            }
        }
        switch event.keyCode {
        case 51, 117: model.deletePressed(); return true
        case 123: model.navigate(.left, groupwise: loupe || option, extend: shift); return true
        case 124: model.navigate(.right, groupwise: loupe || option, extend: shift); return true
        case 126: model.navigate(.up, groupwise: loupe || option, extend: shift); return true
        case 125: model.navigate(.down, groupwise: loupe || option, extend: shift); return true
        case 36, 76: model.requestViewMode(loupe ? .grid : .loupe); return true
        case 53: if loupe { model.requestViewMode(.grid); return true }; return false
        default: break
        }
        guard let ch = event.charactersIgnoringModifiers?.lowercased(), ch.count == 1 else { return false }
        switch ch {
        case "x": model.perform(.reject)
        case "u": model.perform(.undecided)
        case "p": model.perform(.keep)
        case "1", "2", "3": model.perform(.grade(UInt8(ch)!))
        case "6", "7", "8", "9": model.perform(.mark(UInt8(ch)!))
        case "b": model.perform(.toggleBasket)
        case "a": model.autoAdvance.toggle(); model.statusMessage = "Auto-advance \(model.autoAdvance ? "on" : "off")"
        case "k": model.keepBestRejectRest()
        case "y": model.assist.confirmAll()
        case "n": model.assist.dismiss(model.targetIDs)
        case "c": comparing ? model.exitCompare() : model.enterCompare()
        case "z" where comparing: model.toggleCompareZoom()
        case "s" where loupe:
            if shift {
                SoftProof.shared.gamutWarning.toggle()
                model.statusMessage = "Gamut warning \(SoftProof.shared.gamutWarning ? "on" : "off")"
            } else {
                SoftProof.shared.toggle()
                model.statusMessage = "Soft proofing \(SoftProof.shared.enabled ? "on" : "off")"
            }
        case "d": model.enterPhotoEdit()
        case "g": model.requestViewMode(.grid)
        case "e": model.requestViewMode(.loupe)
        default: return false
        }
        return true
    }
    private func handleReview(_ event: NSEvent) -> Bool {
        switch event.keyCode {
        case 53: model.backFromReview(); return true
        case 123, 126: model.moveReviewSelection(-1); return true
        case 124, 125: model.moveReviewSelection(1); return true
        default: break
        }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "d", "e": model.editReviewedPhoto()
        case "g": model.returnToLibrary(grid: true)
        case "x", "u", "p", "1", "2", "3", "6", "7", "8", "9", "b", "a", "k", "y", "n", "c": break
        default: return false
        }
        return true
    }
    private func handlePhotoEdit(_ event: NSEvent) -> Bool {
        switch event.keyCode {
        case 53: model.returnFromPhotoEdit(); return true
        case 123, 126: model.navigate(.left, groupwise: false, extend: false); return true
        case 124, 125: model.navigate(.right, groupwise: false, extend: false); return true
        default: break
        }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "d": model.photoInspectorTab = .develop
        case "m": model.photoInspectorTab = .masks
        case "g": model.returnToLibrary(grid: true)
        case "s":
            if event.modifierFlags.contains(.shift) { SoftProof.shared.gamutWarning.toggle() }
            else { SoftProof.shared.toggle() }
        case "x", "u", "p", "1", "2", "3", "6", "7", "8", "9", "b", "a", "k", "y", "n", "c": break
        default: return false
        }
        return true
    }
    private func handleToolLetterOverKeyOwner(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, let window = event.window, !(window is NSPanel), window.attachedSheet == nil,
              window.sheetParent == nil, NSApp.modalWindow == nil,
              let owner = window.firstResponder, owner is KeyOwningControl, !(owner is TextInputView),
              let doc = model.documents.current else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.intersection([.command, .control, .option]).isEmpty else { return false }
        guard case .tool(let t)? = ToolKeyMap.action(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers ?? "",
                                                     mods: flags.contains(.shift) ? .shift : [], current: doc.tool) else { return false }
        if DocumentTools.shared.document === doc {
            DocumentTools.shared.select(t)
        } else {
            doc.tool = t
            doc.viewport?.cursorDidChange()
        }
        return true
    }
    static func panelViewHasKeyboard(in window: NSWindow?) -> Bool {
        guard let window, let view = window.firstResponder as? NSView, view.window === window,
              view !== window.contentView, !isStrayResponder(view) else { return false }
        var ancestor: NSView? = view
        while let v = ancestor {
            if v is DocumentViewportView { return false }
            ancestor = v.superview
        }
        return true
    }
    static func isStrayResponder(_ view: NSView) -> Bool {
        if view.isHiddenOrHasHiddenAncestor { return true }
        var ancestor: NSView? = view
        while let v = ancestor {
            if v is ThumbnailCollectionView || v.alphaValue <= 0 || (v.layer?.opacity ?? 1) <= 0 { return true }
            ancestor = v.superview
        }
        return false
    }
    static func panelViewKey(_ event: NSEvent) -> Bool? {
        guard event.type == .keyDown, [51, 117, 49].contains(event.keyCode),
              panelViewHasKeyboard(in: event.window), let view = event.window?.firstResponder as? NSView else { return nil }
        if event.keyCode != 49 { return true }
        guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return nil }
        if let button = view as? NSButton {
            if button.isEnabled, !event.isARepeat { button.performClick(nil) }
            return true
        }
        return event.isARepeat
    }
    private func handleDocument(_ event: NSEvent) -> Bool {
        if event.keyCode == 48, Self.panelViewHasKeyboard(in: event.window) { return false }
        if let consumed = Self.panelViewKey(event) { return consumed }
        if DocumentTools.shared.handleKey(event) { return true }
        if event.charactersIgnoringModifiers?.lowercased() == "q",
           event.modifierFlags.intersection([.shift, .option, .command, .control]).isEmpty,
           model.documents.current != nil {
            DocumentChannels.shared.toggleQuickMask()
            return true
        }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var mods: DocumentKeyMap.Mods = []
        if flags.contains(.shift) { mods.insert(.shift) }
        if flags.contains(.option) { mods.insert(.option) }
        guard let action = DocumentKeyMap.action(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers ?? "",
                                                 mods: mods) else { return false }
        let docs = model.documents
        switch action {
        case .tool(let t):
            docs.current?.tool = t
            docs.current?.viewport?.cursorDidChange()
            DocumentTools.shared.publishHint()
        case .panHold:
            if !docs.spaceHeld {
                docs.spaceHeld = true
                docs.current?.viewport?.cursorDidChange()
            }
        case .togglePanels:
            docs.togglePanels()
            if !docs.panelsHidden, let viewport = docs.current?.viewport,
               viewport.window === event.window {
                viewport.claimKeyboardIfStray()
            }
        case .cycleScreenMode: docs.cycleScreenMode()
        case .deleteLayer: docs.current?.deleteSelection()
        default: return false
        }
        return true
    }
}
```

### TesseraCore/Document/DocumentKeyMap.swift

```swift
public static func action(keyCode: UInt16, characters: String, mods: Mods) -> DocumentKeyAction? {
        let ch = characters.lowercased()
        if mods.contains(.control) { return nil }
        if mods.contains(.command) {
            let shift = mods.contains(.shift)
            switch ch {
            case "a" where !shift: return .selectAll
            case "d" where !shift: return .deselect
            case "z": return shift ? .redo : .undo
            case "j" where !shift: return .duplicate
            case "g": return shift ? .ungroup : .group
            case "e" where !shift: return .mergeDown
            case "=", "+": return .zoomIn
            case "-", "_": return .zoomOut
            case "0": return .zoomFit
            case "1": return .zoomActual
            default: return nil
            }
        }
        switch keyCode {
        case 49: return .panHold
        case 48: return mods.isEmpty ? .togglePanels : nil
        case 51, 117: return mods.isEmpty ? .deleteLayer : nil
        default: break
        }
        guard mods.subtracting(.shift).isEmpty else { return nil }
        switch ch {
        case "v": return .tool(.move)
        case "m": return .tool(.marquee)
        case "f": return .cycleScreenMode
        default: return nil
        }
    }
```

### TesseraCore/Document/Tools/EditorTools.swift

```swift
public var group: [DocumentTool] {
        switch self {
        case .marquee, .ellipseMarquee: [.marquee, .ellipseMarquee]
        case .lasso, .polygonLasso, .magneticLasso: [.lasso, .polygonLasso, .magneticLasso]
        case .quickSelect, .wand, .objectSelect: [.quickSelect, .wand, .objectSelect]
        case .rectangleShape, .ellipseShape, .polygonShape, .lineShape: [.rectangleShape, .ellipseShape, .polygonShape, .lineShape]   // B5-11
        case .pathSelect, .directSelect: [.pathSelect, .directSelect]   // B5-11
        default: [self]
        }
    }

public var key: String {
        switch self {
        case .move: "V"
        case .marquee, .ellipseMarquee: "M"
        case .lasso, .polygonLasso, .magneticLasso: "L"
        case .quickSelect, .wand, .objectSelect: "W"
        case .brush: "B"
        case .eraser: "E"
        case .cloneStamp: "S"
        case .heal: "J"
        case .gradient: "G"
        case .crop: "C"
        case .type: "T"
        case .eyedropper: "I"
        case .hand: "H"
        case .zoom: "Z"
        case .rectangleShape, .ellipseShape, .polygonShape, .lineShape: "U"   // B5-11
        case .pen: "P"   // B5-11
        case .pathSelect, .directSelect: "A"   // B5-11
        }
    }

public static func action(keyCode: UInt16, characters: String, mods: DocumentKeyMap.Mods,
                              current: DocumentTool) -> ToolKeyAction? {
        if mods.contains(.command) || mods.contains(.control) || mods.contains(.option) { return nil }
        switch keyCode {
        case 36, 76: return .commit
        case 53: return .cancel
        default: break
        }
        let shift = mods.contains(.shift)
        let ch = characters.lowercased()
        switch ch {
        case "[", "{": return shift ? .brushHardness(harder: false) : .brushSize(larger: false)
        case "]", "}": return shift ? .brushHardness(harder: true) : .brushSize(larger: true)
        case "x" where !shift: return .swapColors
        case "d" where !shift: return .defaultColors
        default: break
        }
        if !shift, ch.count == 1, let d = Int(ch) { return .opacityDigit(d) }
        guard let first = DocumentTool.allCases.first(where: { $0.key.lowercased() == ch }) else { return nil }
        let group = first.group
        if let i = group.firstIndex(of: current) {
            return .tool(shift || current.keyRepeatCycles ? group[(i + 1) % group.count] : current)
        }
        return .tool(first)
    }
```

### Tessera/App/MaskTools.swift

```swift
func handleKey(_ event: NSEvent) -> Bool {
        guard model.viewMode == .loupe, model.developStatus == .ready else { return false }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let ch = event.charactersIgnoringModifiers?.lowercased() ?? ""
        let plain = mods.subtracting([.shift, .option, .capsLock, .numericPad, .function]).isEmpty
        guard plain else { return false }
        if ch == "m", mods.isEmpty, !DevelopTools.shared.cropActive {
            toggle()
            return true
        }
        guard active else { return false }
        switch event.keyCode {
        case 53:                                                     // Esc
            if tool != nil { tool = nil; target = nil } else { setActive(false) }
            return true
        case 51, 117:                                                // ⌫ ⌦
            if let id = list.selectedID { delete(id) }
            return true
        default: break
        }
        switch ch {
        case "o":
            if mods.contains(.shift) {
                let all = MaskOverlayColor.allCases
                overlayColor = all[(all.firstIndex(of: overlayColor)! + 1) % all.count]
                if !overlayOn { overlayOn = true }
            } else {
                overlayOn.toggle()
            }
            return true
        case "x":
            invertSelected()
            return true
        case "[", "]", "{", "}":
            let up = ch == "]" || ch == "}"
            if mods.contains(.shift) {
                brushFeather = min(max(brushFeather + (up ? 10 : -10), 0), 100)
                model.statusMessage = String(format: "Brush feather %.0f", brushFeather)
            } else {
                brushSize = min(max(brushSize * (up ? 1.2 : 1 / 1.2), 2), 600)
            }
            onLoupeChange?()
            return true
        default:
            return false
        }
    }
```

### Tessera/Document/Tools/DocumentTools.swift

```swift
func handleKey(_ event: NSEvent) -> Bool {
        guard let doc = document else { return false }
        if DocumentTransforms.shared.handleKey(event) { return true }   // B5-12: Return / Esc / ⌫ pin
        if DocumentRetouch.shared.handleKey(event) { return true }   // B5-09: ⇧J, and Remove's keys
        if DocumentContentAware.shared.handleKey(event) { return true }   // B5-13: Return / Esc of a move
        if DocumentVector.shared.handleKey(event) { return true }   // B5-11: Return / Esc / ⌫ of the vector tools
        if event.keyCode == 51 || event.keyCode == 117 {
            let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            return mods.isEmpty && clearSelection()
        }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var mods: DocumentKeyMap.Mods = []
        if flags.contains(.shift) { mods.insert(.shift) }
        if flags.contains(.option) { mods.insert(.option) }
        if flags.contains(.command) { mods.insert(.command) }
        if flags.contains(.control) { mods.insert(.control) }
        guard let action = ToolKeyMap.action(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers ?? "",
                                             mods: mods, current: doc.tool) else { return false }
        // B5-10 begin: Return / Esc apply or cancel a text session whose keys reach the tools
        // (focus left the canvas); while the canvas has focus TextInputView owns every key.
        if DocumentText.shared.isEditing(doc) {
            if action == .commit { DocumentText.shared.apply(); return true }
            if action == .cancel { DocumentText.shared.cancel(); return true }
        }
        // B5-10 end
        switch action {
        case .tool(let t): select(t)
        case .brushSize(let larger):
            if doc.tool == .quickSelect {
                quickSize = BrushHUDMath.bracket(size: quickSize, larger: larger)
            } else {
                var b = currentBrush
                b.size = BrushHUDMath.bracket(size: b.size, larger: larger)
                currentBrush = b
            }
        case .brushHardness(let harder):
            var b = currentBrush
            b.hardness = BrushHUDMath.bracket(hardness: b.hardness, harder: harder)
            currentBrush = b
        case .opacityDigit(let d):
            let v = opacityKeys.press(d, at: event.timestamp)
            if doc.tool.paints {
                var b = currentBrush
                b.opacity = v
                currentBrush = b
                say("Brush opacity \(Int((v * 100).rounded())) %")
            } else if doc.primary != nil {
                doc.setOpacity(Double(v * 100), final: true)
            }
        case .swapColors: colors.swap()
        case .defaultColors: colors.reset()
        case .commit:
            if transform != nil { commitTransform() } else if gesture != nil { finishPolygon() } else { return false }
        case .cancel:
            if transform != nil { cancelTransform() } else if gesture != nil { gesture = nil; redraw() } else { return false }
        }
        redraw()
        return true
    }
```

### Tessera/Document/Retouch/DocumentRetouch.swift

```swift
func handleKey(_ event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let ch = (event.charactersIgnoringModifiers ?? "").lowercased()
        if ch == "j", flags == .shift, document != nil {
            if removeActive { deactivate() } else { activate() }
            return true
        }
        guard removeActive, flags.subtracting(.shift).isEmpty else { return false }
        switch event.keyCode {
        case 53:
            if busy != nil || strokeOpen { cancel(); return true }
            if review != nil { endReview(); return true }
            return false
        case 36, 76:
            if review != nil { applyReview(); return true }
            return false
        default: break
        }
        switch ch {
        case "[", "{": options.bracket(larger: false); return true
        case "]", "}": options.bracket(larger: true); return true
        default: return false
        }
    }
```

### Tessera/Document/Vector/DocumentVector.swift

```swift
func handleKey(_ event: NSEvent) -> Bool {
        guard let doc = document, doc.tool.isVector else { return false }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard mods.subtracting([.shift, .function, .numericPad]).isEmpty else { return false }
        switch event.keyCode {
        case 53:   // Esc
            if gesture != nil { cancelDraft(); return true }
            if !pen.isEmpty { pen = PenDraft(); say("Pen path discarded"); redraw(); return true }
            if !selectedAnchors.isEmpty { selectedAnchors.removeAll(); redraw(); return true }
            return false
        case 36, 76:   // Return / Enter
            if !pen.isEmpty { finishPen(); return true }
            return false
        case 51, 117:   // ⌫
            if doc.tool == .pen, !pen.isEmpty { pen.removeLast(); redraw(); return true }
            return deleteSelectedAnchors()
        default:
            return false
        }
    }
```

### Tessera/Document/Text/DocumentText.swift

```swift
func routeSessionKey(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, let s = session, let doc = document, doc.id == s.docID,
              let w = event.window, w === doc.viewport?.window else { return false }
        if w.firstResponder === input { return false }
        if let field = w.firstResponder as? NSTextView, field.hasMarkedText() { return false }
        let flags = event.modifierFlags.intersection([.command, .shift, .option, .control])
        switch event.keyCode {
        case 76 where flags.isEmpty, 36 where flags == .command, 76 where flags == .command:
            apply()
        case 53 where flags.isEmpty:
            cancel()
        default:
            return false
        }
        return true
    }
```

### Tessera/Document/Transforms/DocumentTransforms.swift

```swift
static func keyAction(keyCode: UInt16, modifiers: NSEvent.ModifierFlags) -> KeyAction? {
        let mods = modifiers.intersection(.deviceIndependentFlagsMask).subtracting([.numericPad, .function, .capsLock])
        switch keyCode {
        case 36, 76:
            return mods.isEmpty || mods == [.command] ? .apply : nil
        case 53:
            return mods.isEmpty ? .cancel : nil
        case 51, 117:
            return mods.isEmpty ? .removePin : nil
        default: return nil
        }
    }

func handleKey(_ e: NSEvent) -> Bool {
        guard session != nil, isActive(document),
              let action = Self.keyAction(keyCode: e.keyCode, modifiers: e.modifierFlags) else { return false }
        switch action {
        case .apply: apply(); return true
        case .cancel: cancel(); return true
        case .removePin:
            if case .puppet = session?.op, selectedPin != nil { removeSelectedPin(); return true }
            return false
        }
    }
```

### Tessera/Document/ContentAware/DocumentContentAware.swift

```swift
func handleKey(_ event: NSEvent) -> Bool {
        guard active else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.subtracting(.shift).isEmpty else { return false }
        switch event.keyCode {
        case 53:
            if session != nil { cancel(); return true }
            return false
        case 36, 76:
            if session != nil { apply(); return true }
            return false
        default:
            return false
        }
    }
```

### Tessera/Loupe/LoupeToolOverlay.swift

```swift
func handleKey(_ event: NSEvent) -> Bool {
        let loupe = model.viewMode == .loupe
        guard loupe, ready else { return false }
        if !cropActive, UprightGuideTool.shared.active { return UprightGuideTool.shared.handleKey(event) }
        let ch = event.charactersIgnoringModifiers?.lowercased() ?? ""
        if cropActive {
            switch event.keyCode {
            case 36, 76: commitCrop(); return true                 // Return
            case 53: cancelCrop(); return true                     // Esc
            case 123, 124, 125, 126:                               // arrows nudge the crop
                guard var g = crop else { return true }
                let step = event.modifierFlags.contains(.shift) ? 10.0 : 1.0
                let (dx, dy): (Double, Double) = switch event.keyCode {
                case 123: (-step, 0); case 124: (step, 0); case 125: (0, step); default: (0, -step)
                }
                g.move(dx: dx * g.width / 1000, dy: dy * g.height / 1000, constrain: constrainCrop)
                updateCrop(g, settled: true)
                return true
            default: break
            }
            switch ch {
            case "o": cropOverlay = cropOverlay.next; onLoupeToolChange?(); return true
            case "x": flipCropOrientation(); return true
            case "r": commitCrop(); return true
            default: return true   // swallow culling keys while cropping
            }
        }
        if hslPicker != nil || detailPicking, event.keyCode == 53 {
            hslPicker = nil
            detailPicking = false
            onLoupeToolChange?()
            return true
        }
        if ch == "r", event.modifierFlags.intersection(.deviceIndependentFlagsMask).isEmpty {
            beginCrop()
            return true
        }
        return false
    }
```
