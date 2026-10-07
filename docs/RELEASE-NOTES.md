# Tessera release notes

## Unreleased

### Lens corrections now match Lightroom (ENG-7, ENG-7b)

- **No guessed distortion.** Tessera no longer estimates lens distortion or
  vignetting from the picture's content unless you explicitly choose
  auto-calibration. A photo is corrected by the camera's built-in lens
  correction when the raw file carries one (DNG opcode lists), otherwise by a
  matching lens profile, otherwise not at all. This is what Lightroom does.
- **Built-in corrections always apply.** Like Lightroom, a raw file's built-in
  lens correction is applied even when profile corrections are off or a named
  profile is missing. Built-in corrections stored in camera maker notes (for
  example Fujifilm) are not read yet.
- **Remove Chromatic Aberration is off by default** for new edits, as in
  Lightroom's Adobe Default. Edits that already have it on keep it, and
  Lightroom imports keep their own setting.
- **Missing lens profiles are reported.** Tessera has no Adobe lens profile
  database yet. When an edit names a profile Tessera does not have, Develop
  shows "Lens profile '…' not available — no profile correction applied", and
  so does the export summary. When no profile was available for an exported
  raw, the export summary says so. Lightroom catalog imports list the same
  note under "Approximate translations".
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
