<!-- B5-17a steps 480–484 for apps/mac/ACCEPTANCE.md, section
     "## B5-17. Photo Restoration, painting into channels and alpha display".
     The integrator pastes these under that heading (B5-17b adds 485–490, B5-17c 491–499). -->

Engine backend, a scratch PNG (never a fixture) opened with Edit in Layers. DRUNet (`enhance/drunet-color`) is the
same file JPEG Artifact Removal uses; steps 482–484 need it installed (Settings ▸ AI ▸ Allow model downloads, or the
file placed where the sheet says). The scripted part runs inside the Retouch self-test (step 333,
`--retouch-selftest=<dir>` with `open -g -n … --new-document`): it prints `check Photo Restoration is listed ok`,
`… has one Photo enhancement control ok`, `… shows its limitation ok`, `… needs DRUNet ok`, and without DRUNet
`check restoration on pixels <output>: … ok` / `check restoration on a smart object <output>: … ok` for every
output, then `done, 0 failure(s)`.

480. **Listed with its limitation.** Filter ▸ Neural Filters… on a pixel layer: the list shows Skin Smoothing,
     Colorize, JPEG Artifact Removal and **Photo Restoration** (`document.neural.filter.photoRestoration`). Choose it:
     one slider, **Photo enhancement** 0–1, default 0.50 (`document.neural.photoRestoration.photo_enhancement`);
     under the Output picker the caption reads `Denoise only; GFPGAN excluded and scratch reduction unavailable`.
     Without DRUNet the row carries the **No model** chip.
481. **Missing weights change nothing.** Without DRUNet and with Allow model downloads **off**, choose Photo
     Restoration: the sheet says it needs the `enhance/drunet-color` model and that downloads are off, with a
     **Settings ▸ AI…** button (`document.neural.settings`) that opens Settings on AI. Apply does nothing: no History
     row, no new layer, pixels unchanged, for each of Current layer, New layer and Smart filter. With downloads **on**,
     the button reads **Download and Apply**: the progress row appears (`document.neural.model`) and the filter
     applies by itself when the download completes; the model download is titled `JPEG Artifact Removal, Photo
     Restoration (DRUNet)`.
482. **Each output is one node.** With DRUNet installed, Photo enhancement 0.80: **Current layer** records one
     `Photo Restoration` History row and visibly denoises the layer (inside the selection, if any); ⌘Z restores it
     exactly. **New layer** adds one layer above named `<layer> (Photo Restoration)`, the original unchanged, one row; ⌘Z removes
     it. **Smart filter** (no selection) converts the layer into a smart object with a `Photo Restoration` smart
     filter in the same single row; ⌘Z returns the pixel layer.
483. **Re-open a smart filter.** Double-click the `Photo Restoration` smart filter row under the layer: the sheet opens
     as **Edit Photo Restoration** on that filter with Photo enhancement at 0.80 and Output fixed to Smart filter.
     Change it to 0.30 and click **OK**: one `Edit Smart Filter` row; double-click again shows 0.30.
484. **Smart objects.** Select a smart object and choose Filter ▸ Neural Filters…: New layer is unavailable (its
     reason is listed). Photo Restoration with **Current layer** or **Smart filter** adds a re-editable smart filter
     (one row each), the smart object's contents unchanged; without DRUNet neither adds a smart filter nor a row.
