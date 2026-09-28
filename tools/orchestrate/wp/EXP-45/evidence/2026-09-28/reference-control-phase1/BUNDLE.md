# EXP-45 phase 1 portable evidence

This is a copy of the completed, bounded reference-control diagnostic. `RESULTS.md` and `MANIFEST.json` are byte-identical to the original external report and manifest verified before packaging. `PROVENANCE-RECOVERED.md` records subsequently recovered PPM/PGM and cjpeg commands without inventing uncaptured exits. `source-reviews/` retains the independent probe/reference and denominator source audits. `SHA256SUMS` covers the portable files.

The Google libultrahdr source checkout, build objects, compiled library/application, and compiled native probe binaries remain only in the external validation directory. The copied `MANIFEST.json` records their hashes and original paths. Small source-input, reference-output and single-bit diagnostic fixtures are present under `fixtures/`. `fixtures/retained-A-4-stops.jpg` is a copy of the previously frozen A output for direct comparison. No file in this bundle changes Tessera production source or the original ImageIO assertion.

The result is **not** a positive ImageIO host control: the upstream Google reference file decodes above 8 in the reference codec but its ISO auxiliary is absent to both ImageIO and Core Image on A. The one-byte MPF representative-bit copy remains absent. The original A 4-stop ImageIO core failure stays open.
