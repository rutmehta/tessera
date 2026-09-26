# Licensing Decision

Decided 2026-09-25 after a review of how Adobe, Capture One, DxO, Affinity, Pixelmator, Apple, darktable, RawTherapee, digiKam and Photo Mechanic handle decoders, encoders and licences, and how open-core projects (Krita, Blender, Signal, Grafana, Sentry, Plausible) monetise. Not legal advice; have counsel review before the first public release.

## Decision
- **Engine and desktop app: Apache-2.0.** Patent grant suits code that touches image formats. Keeps the Mac App Store, later commercial builds, and third-party embedding all open.
- **Contributions: DCO (Developer Certificate of Origin)**, no CLA. Under Apache-2.0 a CLA adds nothing we need.
- **Cloud AI services: proprietary.** Calling an HTTP API is not linking; no open-source licence here reaches the server code.

## Dependency policy (enforced by `cargo-deny` in CI)
| Allowed | Condition |
|---|---|
| MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0, CDDL-1.0 | none |
| LGPL-2.1 / LGPL-3 (e.g. `rawler`, libheif) | source stays public, or the library ships as a `.dylib` in any closed build |
| LibRaw | taken under its **CDDL** option, never the LGPL one (CDDL is fine with static linking) |
| **Banned:** GPL-2/3, AGPL-3, including `jpegxl-rs`, `jpegxl-sys`, `exiv2` bindings | a single GPL crate makes the whole statically linked Rust binary GPL |

JPEG XL: decode with `jxl-oxide` (MIT/Apache), encode through our own thin bindings to libjxl (BSD-3), which is what Adobe's DNG SDK ships.

## Optional Lensfun data pack

Lensfun's calibration XML database is CC-BY-SA-3.0, not Apache-2.0.
It is not compiled into or bundled with the engine. Users may download the
upstream data pack separately from https://lensfun.github.io/ and load its XML
through `lens::ProfileDatabase::from_lensfun`. Attribution: Lensfun contributors,
https://github.com/lensfun/lensfun, licensed under
https://creativecommons.org/licenses/by-sa/3.0/ . Keep the upstream copyright,
attribution and license notices with any redistributed pack. Modified calibration
data must retain attribution, identify changes and be distributed under the same
license. The engine implements its own reader and does not link Lensfun code.
`lens::LensDataPack::new(app_dir)?.resolve()` explicitly downloads on demand,
verifies SHA-256, and atomically caches the original archive under
`app_dir/lens/<sha256>.tar.gz`. Construction does not access the network.
The pinned source is Lensfun v0.3.4, commit
`101c745e847a5de4a1e569a94368ce2027198598`, at
https://codeload.github.com/lensfun/lensfun/tar.gz/101c745e847a5de4a1e569a94368ce2027198598 .
Its SHA-256 is `a11cbe6aeec657839540448b253217c25d20b7a45b6aebfef406f7239933c7a6`.
The returned `LoadedLensDataPack.spec` exposes version, attribution, source and
license for display. Attribution includes Lensfun contributors and original
PTLens data by Tom Niemann. The original archive retains upstream license and
copyright notices; source-code members are never extracted, executed or linked.
Only calibration XML is interpreted through the existing loader. Unsupported
or uncalibrated lenses are reported in `skipped`, not silently approximated.
Archive loading is bounded (16 MiB compressed, 64 MiB expanded, 2 MiB per XML),
rejects links/unsafe paths, and rechecks integrity on cache reuse. Updates require
an explicit new trusted manifest rather than an automatic mutable download.

Adobe `.lcp` files are user-supplied only; no Adobe profile assets are shipped or
downloaded automatically. User-created JSON calibration profiles are stored
separately from third-party packs and retain their source provenance obligations.

## Neural-filter model table (M3-21)

Neural-filter weights and ONNX exports must be Apache-2.0, MIT, BSD-2-Clause,
or BSD-3-Clause, including inherited third-party terms. A repository's generic
license badge is not sufficient. No weights are bundled in the repository.

| Model | License / decision | Use |
|---|---|---|
| DDColor paper-tiny, `edgetools/ddcolor` export | Apache-2.0 upstream and export declaration; immutable revision and SHA-256 in `crates/ml-runtime/models.toml` | Colorize, explicitly CPU-only because this graph fails the strict CoreML partition guard |
| DRUNet color, `cszn/KAIR` / `synthscript/drunet-color-onnx` | MIT upstream and export; existing pinned registry entry reused | JPEG Artifact Removal and whole-frame denoise-only restoration |
| GFPGAN v1.4, TencentARC | **Excluded by the round-2 scope decision:** StyleGAN2/NVIDIA non-commercial terms in its lineage; reviewed ONNX declares `license: other` | Not registered, downloaded by the runtime, or executed; no face-restoration model ships |
| CodeFormer / GPEN | Not acceptable under this package's permissive-only policy | Not used as substitutes |
| Scratch reduction | No approved model selected | Not implemented; this is not a claim that no permissive model exists |

Skin Smoothing is a deterministic algorithm and requires no model weights.
`Photo Restoration (no face model)` retains only DRUNet denoising and is omitted
from the three-entry shipped catalog. Enhance face and Scratch reduction reject
nonzero values rather than silently doing nothing. See
[`ml-filters/MODELS.md`](../crates/ml-filters/MODELS.md) for pinned upstream/export
license evidence and hashes, including the GFPGAN third-party exceptions. A local
re-export does not remove those restrictions. Keep applicable Apache/MIT notices
with any future redistributed models.

## Why not GPL
darktable and RawTherapee are GPL-3 and it works for them, but: GPL has blocked Mac App Store distribution in practice (VLC was pulled in 2011 and returned only after relicensing), selling a commercial licence later requires a CLA and removing every GPL dependency, and companies will not embed a GPL engine behind the MCP tool API. Moving Apache → GPL later is easy; the reverse is not.

## Enforcement reality
Copyleft enforcement is civil and mostly about source release, not damages (Orange paid €860k after 13 years; AVM settled for source plus costs). Problems are found by licence scanners during fundraising or acquisition due diligence, by users requesting source, or by copyright holders complaining to app stores. Black Duck's 2026 audit found licence conflicts in 68% of codebases, rising because of AI-generated code, which is exactly our situation and why the CI gate exists.
