# EXP-45: next-step decision (source/evidence only)

## Disposition

The original ImageIO four-stop acceptance remains failed. Phase 10 rules out destination-context headroom as a repair: target 16 leaves A and the independent split-map control at approximately 7.98376 with `CGImage` headroom 8, while the uniform control is approximately 15.95158/headroom 16. No serializer fix is justified by current evidence.

There is one useful, still-falsifiable interaction to isolate before calling the discrepancy irreducible: **registration of a sharp gain-map boundary against edges in the SDR base**. The frozen split control changes gain-map content, but it does not reproduce A's exact base geometry. The A source fixture has a dark/bright boundary at x=40 and its gain JPEG is a 0/255 step at x=40 (`/tmp/tessera-exp45-gain-pattern-source-audit.md`; source `crates/export/tests/gain_map.rs:671-724`). The phase-7 reference base is a three-band image (64 at x=0–25, 128 at x=26–52, 255 at x=53–79), while its split map changes at x=40. Thus, phase 7 establishes that a split map can produce ImageIO ~8 even in a different valid container, but does not test whether the aligned A base/map edge matters. Phase 7's exact split is independently qualified: 640 zeros/640 255s, reference decoder peak 16.0, and same primary JPEG/ICC/ISO as its uniform control apart from MPF auxiliary-size bookkeeping (`tools/orchestrate/wp/EXP-45/evidence/2026-09-28/reference-control-phase7/RESULTS.md`, `pgm-change.json`, `static-qualification.json`).

## Smallest discriminating control

Make one scratch copy of the retained A JPEG and change only the auxiliary gain map's vertical step position from x=40 to x=32 (or x=48), retaining exact values 0/255, dimensions, ISO values/flags, base JPEG bytes, ICC, and marker policy. Update only unavoidable auxiliary length/MPF size bookkeeping. Before ImageIO, require the pinned libultrahdr reference decoder to still produce an actual >8 bright-region peak, and structurally validate the decoded auxiliary values, association, and unchanged primary codestream/profile. Run original A and the shifted-map copy through the existing ImageIO probe under the same frozen options.

A change in ImageIO headroom/reconstruction that follows the shift would support a base/gain registration interaction. No change would falsify that specific alignment explanation for this fixture; it would not prove a general ImageIO limitation. If safe byte-exact repacking of A's ISO/MPF associations cannot be independently qualified, stop rather than approximate the file or alter production code.

## Already tested; do not repeat

- ISO flag/common-vs-explicit admission, rational scaling, MPF representative bit, APP marker order, full primary ICC swaps, white-level difference, and larger dimensions: phase 1–8 reports under `tools/orchestrate/wp/EXP-45/evidence/2026-09-28/`.
- An independent libultrahdr/cjpeg split map reproduces the ~8 ImageIO result; uniform map gives ~16, although both reference-decode to 16. Phase 7 `RESULTS.md` records this and explicitly says it does not identify Apple's algorithm.
- HDR-only `kCGComputeHDRStats` did not change the result (phase 9). Destination CGContext target 8 changes only the uniform control; target 16 does not lift A or split (phase 10 `RESULTS.md`).
- Phase 10's 640×128 nearest-neighbor controls exclude a simple small-image-size explanation. Its original 4-pass/1-fail gate remains unchanged.

A codestream-only explanation is lower priority: A's gain map uses Rust `jpeg_encoder::Encoder` quality 100/Luma (`crates/export/src/gain_map.rs` in the preserved source audit), while the phase-7 map uses pinned libjpeg-turbo `cjpeg`; however phase 7 already reproduces the failure without A's encoder, so that encoder is not a necessary cause. A same-input cross-encoder test could characterize contribution, but is less discriminating than the exact-base registration control and should not be the next experiment.

## Contract limit

Apple documents `CGImage.contentHeadroom` as a property and describes HDR image metadata/stats APIs, but the public material reviewed does not specify the required per-pixel output or reported-headroom algorithm for this ISO 21496-1 nonuniform-gain case. ISO's published abstract says the standard defines metadata, how to specify it, and how to apply it; it does not define Apple's ImageIO implementation behavior. Therefore, even a negative registration control would leave a missing normative/vendor contract fact. Do not call this a platform cap or file-format violation without that contract.

Sources: [Apple CGImage.contentHeadroom](https://developer.apple.com/documentation/coregraphics/cgimage/contentheadroom), [Apple ImageIO image properties](https://developer.apple.com/documentation/imageio/individual-image-properties), [ISO 21496-1:2025](https://www.iso.org/standard/86775.html).
