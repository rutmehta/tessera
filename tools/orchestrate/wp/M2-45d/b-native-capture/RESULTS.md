# Fresh JPEG and independent ISO controls: B native diagnostic

This is a separate bounded diagnostic after the frozen core gate failed (1 passed / 4 failed). It does not replace that gate or accept the encoder. B macOS 26.1 (25B78), M4 Max; no production code or assertion changed. Exactly one canonical test ran after a test-only artifact-copy block was inserted before native readback. Its expected failure was retained (exit 101). Existing decode-probe.m compiled successfully and decoded the retained JPEG plus two pinned Google Skia ISO controls with PROBE_OPTIONS=none; no host gates ran.

Fresh JPEG SHA256: 24b13e14776eac350f95aa83d5cc5fa3df0ac306beb6172f0238646156827267.

| Layer of evidence | Fresh output | Two independent Skia controls |
|---|---|---|
| ISO auxiliary discovery | absent | absent for both |
| Actual SDR pixels | finite, opaque, provider bytes materialized | finite, opaque, provider bytes materialized |
| Actual HDR-request pixels | finite, opaque, identical to SDR | finite, opaque, identical to SDR for both |
| Native HDR reconstruction | not observed; content headroom 1, peak 1 | not observed; content headroom 1, SDR/HDR delta 0 |

Control RGB peak 1.0886 in linear sRGB is not evidence of HDR reconstruction: SDR and HDR pixels are identical. The test fixtures' color conversion can exceed the nominal sRGB range. This result identifies a recognition/reconstruction limitation on this B host for the specific tested controls. It does not establish universal macOS 26.1 incompatibility, certify our encoder, or explain an OS implementation cause.

The retained fresh output's independent base+gain reconstruction passes all five unchanged patch criteria. The frozen Rust test reached its native helper after independent MPF/ISO parsing, actual base/gain decoding, original 4% patch checks, and ExifTool extraction. Separately, existing Machine A /opt/homebrew/bin/djpeg decoded retained primary.jpg/gain.jpg, then inverse-sRGB * 2^(2*gain/255) reconstructed expected PQ-reference values; worst normalized patch error 1.01563%. See independent-reconstruction.json. This is mathematical reconstruction, distinct from native HDR display capability.

Native B HDR-request values fail bright-patch reference reconstruction: x40 normalized error up to24.7116%, x56 up to72.1069%, x72 up to74.7763%. Black and diffuse gray pass, while highlights remain SDR. See native-reconstruction.json. No whole-frame 4% pass is asserted. Earlier A diagnostic whole-frame discrepancy remains a separate result, not superseded by this B failure.

Fresh artifacts include canonical JPEG, independently extracted primary/gain JPEGs, expected PQ-reference patch values, actual native SDR/HDR RGBA float32 buffers (80x16, four channels, little endian, extended linear sRGB, opaque alpha). Both control files and their actual pixel buffers are retained. Exact controls are pinned Google Skia c6bb2106c13b23d67f95562c99ade38d4b0e2eb1; provenance.json and LICENSE accompany them.

Twenty-four remote files were copied and SHA256-verified; sha256.json includes the compiled probe's hash, but that host-specific executable is intentionally not published in Git. The source, compilation command, logs and raw pixels suffice for reproduction. Source input archive hash was 4b79b1189d7ef9c94199ba523db018395518d9ce4c31c85892c63e677a57055d. capture.patch, manifest.json and ownership-note.txt distinguish the capture source from the authoritative frozen snapshot.

B heavy slot released. B remains detached at accepted base69 with the 13-file uncommitted snapshot plus test-only capture patch preserved; no reset/cleanup. A's original 13 source hashes remain unchanged. Next validation is fresh core output on A, which previously reconstructed the independent controls; no A build has run yet.
