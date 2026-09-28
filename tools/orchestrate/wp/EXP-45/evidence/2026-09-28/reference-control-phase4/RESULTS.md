# EXP-45 phase 4: exact-white independent control

Date 2026-09-28, same A host macOS 26.6.2 (25G83), arm64. This bounded experiment asks whether the previous admitted Google control's SDR bright patch of 240/255 (linear SDR peak ~0.871) could explain why it reached ImageIO headroom16 while the retained A file's white patch 255/255 reached only ~8. The original phase-1/2/3 fixtures and Tessera source/assertion were unchanged. `COMMANDS-AND-PROVENANCE.md`, `base-ppm-change.json` and `MANIFEST.json` record producer options, source/byte hashes, binaries and direct exits.

## Qualifying the single new reference file

Only the right-hand 27×16 patch of the prior 80×16 PPM was changed: all three RGB sample bytes of its 432 pixels went from 240 to 255 (1,296 changed raw sample bytes). The earlier 64 and 128 patches, grayscale gain JPEG, cap-16 metadata, cjpeg quality100/1×1, pinned `libultrahdr v1.4.0` commit `d52a0d13814ca399fc8a07e23de1d2c63f0e8404`, and its API-4 encode route stayed the same. The reference output in default ISO-only common-denominator form is `reference-16-bright255-common.jpg`, SHA-256 `b3da8f235a4551adba8e7f67e73d174b23fb093c79b071e05e9ef0a31fd082e7`, 1,894 bytes. The pinned decoder's documented linear RGBAHalfFloat output has an actual bright-center RGB of **16.0, 16.0, 16.0**, alpha1 (not just metadata saying16).

The same proven scratch transformation made `reference-16-bright255-explicit.jpg`, SHA-256 `61824f8eb85beb703d17c486e4c146c755ab6b6726213d4502aa6bf3b867f351`, 1,918 bytes. `make_explicit.py` checks the exact common source SHA, parses MPF/ISO fields, changes the auxiliary flag `0x48` to semantically equivalent explicit-denominator `0x40`, expands ISO APP2 by24 bytes and updates only the MPF auxiliary size 416→440. It does not add the representative-image bit or change compressed imagery. `static-qualification.json` confirms ISO v0, numeric values `0,4,0,4,1,0,0`, byte-exact ExifTool MPF extraction at offset1478, and the same ICC SHA-256 `be1eccdf7ba1b4c2b13dae337d96b87040fc9206d2d388f7fbed8187cf2c5a6c` as the old admitted Google240 control. The explicit copy's reference decoder output is **byte-identical** to the common original (10,240 bytes, SHA-256 `f8ed048c2b04f259ff1a65d77a3e35c17556d355c0322dd279a9d59d3240a19e`), with the same actual 16.0 bright center.

## Same-host native comparison

The unchanged phase-1 scratch ImageIO and software Core Image probe binaries were run on the old admitted Google240 control, the new Google255 explicit copy, and the unchanged retained A four-stop JPEG **in the same invocation**. Raw ImageIO stdout (`native/imageio.stdout`) starts with four `too few samples` lines before a valid JSON array; the prefix and parsed JSON are separately retained without rewriting raw output. Both direct process exits are 0. Every file had ISO auxiliary present and finite, opaque decoded pixels.

| File | ImageIO SDR peak | ImageIO HDR peak | ImageIO headroom | Core Image HDR peak |
|---|---:|---:|---:|---:|
| Google240 explicit (old) | 0.871337890625 | 13.9340171814 | 16 | 13.941894531 |
| Google255 explicit (new) | 1 | **15.9515762329** | **16** | 16.000003815 |
| Retained A 4-stop | 1 | 7.9837622643 | 8 | 16.000003815 |

The exact-white reference demonstrates ImageIO on A can read a recognized independent ISO JPEG whose SDR white patch is 1 and reconstructed HDR white patch is approximately16, though ImageIO's pixel is 15.9516 rather than exactly16. This eliminates the previous 240/255 versus 255/255 bright-patch difference as the cause of A's ~8 ImageIO readback. It does **not** resolve the remaining A-specific difference, prove general host behavior, validate the Tessera encoder, or waive the original frozen ImageIO core failure (4 pass/1 fail). No further container/metadata modifications or runtime probes were performed.
