# Tessera release notes

## Unreleased

### Fujifilm built-in lens corrections (ENG-8)

- **Fujifilm raw files are now corrected like in Lightroom.** Fujifilm X-series
  cameras store their own distortion, vignetting and lateral chromatic
  aberration correction in the raw file. Lightroom always applies it, and so
  does Tessera now, in every lens setting, including with profile corrections
  off. On the X-E2S sample in this repo (XF18-55mm at 31.5 mm) the picture is
  slightly enlarged toward the corners, pincushion distortion is removed and
  the corners are about 5% brighter. The camera's own JPEG now lines up with
  the raw much more closely.
- **Existing Fujifilm edits shift slightly.** Crops, masks, healing spots and
  Upright adjustments on Fujifilm raws now land on corrected content, which
  moves by up to about 0.5% of the image width. Review crops and local
  adjustments on Fujifilm photos.
- **Fujifilm Smart Previews need rebuilding.** A Smart Preview of a Fujifilm
  raw made by an earlier version opens as Stale. Its offline edits still
  synchronize to the original; once synchronized, Build Smart Preview
  rebuilds it with the correction.
- **Grid previews are re-rendered once more.** Cached previews from earlier
  versions are rebuilt the next time each photo is shown.
- **Everything you draw lands where you draw it.** The correction is applied
  at the very start of the pipeline, as Lightroom does with built-in
  profiles, so brushes, radial and linear gradients, Object and Person
  clicks and boxes, healing spots, AI masks, crops and Upright all work on
  the corrected picture you see, and mask overlays and handles sit on the
  adjustment. (Crop rectangles saved on Fujifilm raws before this release
  were drawn on the uncorrected picture; see "Existing Fujifilm edits
  shift slightly" above.)
- **AI-masked exports work.** Exporting, printing or placing in a document a
  Fujifilm photo whose edit uses AI masks works at the usual speed. With a
  lens profile, lens auto-calibration or manual distortion on any raw, such
  exports no longer fail with "AI masks with lens warps require a
  hook-aware lens renderer"; they render the masks before the warp, as
  Develop does. AI masks are also computed in that geometry, so they
  stay on their subject with a profile or auto-calibration. Masks created
  in Develop are recomputed when a photo is next opened; masks imported
  from Lightroom are used as stored.
- **DNG files with a built-in distortion correction look right in Develop.**
  Develop dropped the distortion part of a DNG's built-in correction (its
  opcode warp) while exports applied it. Develop now shows it too.
- **Some Fujifilm Smart Previews may need rebuilding later.** If a later
  version computes the built-in correction differently, a Smart Preview made
  with the earlier result opens as Stale and can be rebuilt from the
  original, as above.
- **Other cameras are unchanged.** Sony, Panasonic and Olympus/OM raws also
  carry correction data in their maker notes. Tessera does not apply it yet:
  Lightroom applies Sony's data only for some camera and lens combinations,
  and there are no Panasonic or Olympus samples to verify against.

### Faster exports and prints of Lightroom-process edits (ENG-10)

- **Much faster.** Exporting or printing a photo whose edit came from
  Lightroom is about 8 times faster for a 16-megapixel raw file. A
  full-size or 2048-pixel export takes about 1.4 s instead of more than
  12 s, and a 4x6 print about 0.4-0.6 s instead of 6 s. Develop also
  redraws these edits several times faster.
- **Matches Develop exactly.** Exports and prints of these edits are now
  drawn by the same renderer as Develop. With lens auto-calibration
  selected, re-exporting a Canon CR3 or Fujifilm X-Trans photo gives a
  slightly different file than before. In this repo's samples, up to
  about 2% of the values change, all toward what Develop shows. With the
  default lens settings, full-size files are unchanged.
- **Small prints look crisper.** A print smaller than the photo is drawn
  the way Develop draws the photo at that size, as prints of Tessera's own
  edits already were. Sharpening and edge contrast are therefore somewhat
  stronger than before on small prints. File exports still render at full
  size and then resize.

### Lens corrections now match Lightroom (ENG-7, ENG-7b, ENG-7c)

- **No guessed distortion.** Tessera no longer estimates lens distortion or
  vignetting from the picture's content unless you explicitly choose
  auto-calibration. A photo is corrected by the camera's built-in lens
  correction when the raw file carries one (DNG opcode lists), otherwise by a
  matching lens profile, otherwise not at all. This is what Lightroom does.
- **Built-in corrections always apply.** Like Lightroom, a raw file's built-in
  lens correction is applied even when profile corrections are off or a named
  profile is missing. Fujifilm's maker-note corrections are read since ENG-8
  (see above).
- **Remove Chromatic Aberration is off by default**, as in Lightroom's Adobe
  Default. Edits that saved the setting keep it, and Lightroom imports keep
  their own setting. Documents that never stored the setting now render with
  it off: new edits, but also old documents from early Tessera versions,
  partial or scripted recipes, and XMP sidecars without `AutoLateralCA`.
- **Missing lens profiles are reported.** Tessera has no Adobe lens profile
  database yet. When an edit names a profile Tessera does not have, Develop
  shows "Lens profile '…' not available — no profile correction applied", and
  so does the export summary for that photo. A photo with no lens profile at
  all (the usual case) is not flagged on export. Lightroom catalog imports
  list the note under "Approximate translations".
- **Existing edits may shift slightly.** Some photos used to receive a guessed
  distortion correction. In this repo's test set that included a Canon EOS M50
  and a Fujifilm X-E2S sample. On those photos the image is now uncorrected,
  so the content under an existing crop, mask, healing spot or Upright
  adjustment moves by up to a few percent of the image near the corners.
  Smart Previews and originals change the same way. Review crops and local
  adjustments on such photos. Choosing auto-calibration explicitly brings the
  old estimate back.
- **Grid previews are re-rendered once.** Cached previews from earlier versions
  are not reused. They are rebuilt the next time each photo is shown, and the
  old ones are evicted automatically.
- **Some Smart Previews need rebuilding.** A Smart Preview made by an earlier
  version for a raw with built-in DNG lens corrections, with profile
  corrections off or with a missing named profile, opens as Stale. Its offline
  edits still synchronize to the original. Once synchronized, Build Smart
  Preview rebuilds it from the original.
- **No new Smart Previews for some DNGs with profile corrections off.** For a
  raw whose built-in correction is stored in OpcodeList3 (some DJI and Leica
  DNGs), Tessera can't yet build a Smart Preview when profile corrections are
  off. Building reports that the original is required, as it already did with
  the default lens setting. Edit those photos with the original attached.
