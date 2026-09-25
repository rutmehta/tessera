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

## Why not GPL
darktable and RawTherapee are GPL-3 and it works for them, but: GPL has blocked Mac App Store distribution in practice (VLC was pulled in 2011 and returned only after relicensing), selling a commercial licence later requires a CLA and removing every GPL dependency, and companies will not embed a GPL engine behind the MCP tool API. Moving Apache → GPL later is easy; the reverse is not.

## Enforcement reality
Copyleft enforcement is civil and mostly about source release, not damages (Orange paid €860k after 13 years; AVM settled for source plus costs). Problems are found by licence scanners during fundraising or acquisition due diligence, by users requesting source, or by copyright holders complaining to app stores. Black Duck's 2026 audit found licence conflicts in 68% of codebases, rising because of AI-generated code, which is exactly our situation and why the CI gate exists.
