# LR-12 — partial handoff; private baseline blocked

Machine B, `local/rut-build`, commits on top of `6682ca82`; no rebase or push.
This is **not a completed LR-12 delivery**. The renderer admission policy remains
unchanged so that the required private before measurement can be taken first.

## Access blocker

The supplied scratch catalog exists. No `.lrdata` preview bundles were found
under the supplied scratchpad. The task also prohibits access under the user's
Lightroom directory. An asynchronous question requests accessible read-only
Smart Previews and standard Previews bundle locations, or an explicit read-only
exception for those source bundles. No access to that prohibited directory was
attempted. The live Tessera library and application-support directory were not
accessed. No GUI was launched.

The LR-12 app directory was not created. No private contact pairs were generated.
The catalog-only audit uses the supplied copy and does not resolve/open original
images or preview assets. Private strings, source paths, names, and recipe values
are not emitted by the audit.

## Completed fix: split-preview import sheet

`PreviewIndex` already handles current-digest split JPEG levels as well as legacy
containers. Two bridge callers still tested only `lrprev_path(...).is_file()`:

- The import summary's preview count.
- The fidelity sampler's preferred candidate pool.

Both now call `has_preview`, reusing the existing implementation. The synthetic
bridge regression converts six legacy preview fixtures into split levels, then
checks the real import summary and three successful fidelity comparisons.

RED: the summary reported 0 instead of 6. GREEN: the full `tessera-ffi --test
lrcat` suite passed: 17 passed, 0 failed, 1 existing ignored test, no test filter.
The fidelity renderer itself is not fixed yet; real recipes can still fail.

## Catalog-only field audit

This is an inventory across **all 21,656 catalog recipes**, not the 19,726
confirmed offline proxies, and not a count of images failing an app path.
Each non-default field is tested in isolation against the CPU settings validator;
Adobe profile identity/display-transform handling is neutralized as the renderer
does. Counts overlap. No optional operator values or names are included.

| CPU-rejected setting field | Image count |
| --- | ---: |
| `camera_profile/look` | 17,032 |
| `output/hdr_headroom_stops` | 17,133 |
| `effects/lens_blur` | 417 |
| `output/hdr` | 372 |
| `locals/retouch` | 289 |

Retouch can be admitted by a caller that registers a renderer. HDR presentation
is stripped or consumed in some callers. Develop sanitizes other settings too.
These field counts therefore must not be presented as Develop failure counts.
Missing lens dependencies are not detected by this settings-only audit.

Reproduction, with the task's copied catalog supplied through the environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-8"
export CARGO_BUILD_JOBS=6
export RAYON_NUM_THREADS=6
cargo test --release -p tessera-ffi --lib \
  lrcat::lrcat_profile::lr12_recipe_audit_from_env \
  -- --exact --ignored --nocapture
```

The opt-in audit passed. It requires `TESSERA_LRCAT_PROFILE` to canonicalize
inside `/private/tmp`, suppresses dependency output, and prints aggregate typed
setting-field counts only. It does not instantiate an Engine or app library.

## Tests-first work awaiting fixes

All four synthetic tests below failed against unchanged rendering code, with
valid fixture input and the expected unsupported-setting/lens errors:

- `lr12_linearraw_ignores_mosaic_controls_without_losing_rgb_edits`
- `lr12_unresolved_creative_look_does_not_hide_linearraw_photo`
- `lr12_unavailable_named_lens_does_not_hide_linearraw_photo`
- `lr12_hdr_presentation_does_not_block_linearraw_scene_pixels`

They are intentionally committed RED in `07337d42`. The branch is not green.
No existing pixel or import golden was changed.

## App path findings for continuation

- Imported Lightroom DNGs use `open_develop_session`, not the journal-backed
  `open_smart_preview_develop_session` used for Tessera-generated previews.
- Develop selects `session_renderable`, builds real rendering resources and
  mask hooks, and eventually calls the image-core camera-linear path.
- Imported-proxy grid/loupe generation in `preview.rs` passes the full recipe
  directly to pipeline-adobe or pipeline-cpu. The journal-backed thumbnail
  implementation is a different path and should not stand in for this audit.
- Fidelity passes full recipe settings to image-core.
- Export opens an external `CameraLinearProxy` and allows export at its size;
  native generated proxies retain a separate full-quality rejection.
- `CameraLinearProxy::validate_prefix` rejects external mosaic denoise;
  `resident_tail_plan` declines all external DNGs and catalog-oriented sources.
- Image-core camera-linear admission rejects depth/blur/retouch, unsupported
  masks, then applies a broad CPU settings validator. That validator obscures
  individual fields behind the generic non-default-operator error.
- A shared source-specific plan must preserve the saved recipe and distinguish
  external LinearRaw sources from immutable generated RAW-prefix caches.

## Required remaining work and gates

| Deliverable | Status |
| --- | --- |
| Every offline proxy: Develop admission before → after | Not measured |
| Every offline proxy: thumbnail/loupe admission before → after | Not measured |
| Every offline proxy: export admission before → after | Not measured |
| Deterministic 200 full renders validating admission predictions | Not run |
| Optional-operator fixes, diagnostics, GPU admission | Not implemented |
| Same 12 private pairs through real app Develop | Not run |
| Full requested release tests after touched-crate clean | Not run |
| Workspace clippy `-D warnings` | Not run |
| `cargo fmt --all --check` | PASS |
| FFI generation, Swift gate, strict release app build | Not run |

Commits so far:

- `c5f6ae92` — RED split-preview import sheet regression.
- `b47a482b` — split-preview summary/fidelity selection fix.
- `07337d42` — RED LinearRaw operator regressions, aggregate recipe audit,
  and test formatting.

The pre-existing generated `apps/mac/Sources/TesseraFFI/TesseraFFI.swift` change
was preserved and is not included in these commits. Cargo.lock, dependency
manifests and board.json are unchanged. All LR-12 commits use the requested
co-author trailer. This documentation commit follows the code commits above.
