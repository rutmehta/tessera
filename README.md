# Tessera

Tessera is a macOS photo workflow and layered-image engine built around tiled images. Its Rust workspace handles raw decoding, nondestructive development, a rebuildable catalog, culling, local masks, export, on-device ML and document composition; an AppKit/SwiftUI app presents the library and Develop workflow through a UniFFI bridge, while a headless CLI and MCP server expose engine operations. The layered editor's engine is under active development and its full Mac document UI is not yet shipped; this is not a claim of Lightroom or Photoshop feature parity.

## Screenshots

Screenshots are not checked in yet. Add real grid/Develop and layered-editor screenshots here once those flows are verified; use the Mac app's [design system](apps/mac/DESIGN.md) for visual context, not as a substitute for product screenshots.

## What works

- **Catalog:** folder indexing (SQLite/FTS5), sidecar-first recipes and XMP, albums, smart albums, search, previews, and a read-only Lightroom catalog import path that writes Tessera-side data.
- **Culling:** decisions, grades, marks, burst groups, compare, defect review, face strips and assisted suggestions requiring confirmation.
- **Develop:** raw pipeline with CPU reference and GPU rendering, Basic/tone/colour/detail/effects, lens and geometry controls, history, presets, colour-managed display and soft proofing. See [current status](docs/STATUS.md) for limitations.
- **Masks:** procedural, brush and range masks, AI subject selection and local adjustments; some local/AI-mask export paths and fast GPU paths still have gaps.
- **AI:** on-device faces, embeddings, segmentation, depth, quality scoring, enhancement and captions/OCR; agentic base edits can use local or configured external planners. Model weights are not bundled; supported models are pinned in the [registry](crates/ml-runtime/models.toml) and downloaded/verified when explicitly resolved or first needed by a feature.
- **Export and print:** JPEG/PNG/TIFF export with ICC/XMP and batch support; Mac export dialog and print/soft-proof paths. This does not imply CMYK or arbitrary printer-profile export.
- **Layered editing:** compositor, PSD/PSB interchange, filters, brushes, selections and document tools exist in the engine. The Mac layered-document UI and remaining text/vector/performance integration are tracked as work in progress, not presented as a finished editor.

## Build on macOS

Install Xcode (macOS 15+; the Mac package documents Xcode 26 / Swift 6.3), Xcode command-line tools, and the stable Rust toolchain with `rustfmt` and `clippy` (`rust-toolchain.toml`). Install `cargo-deny` for the CI script. From the repository root:

```sh
# Keep Cargo's target directory outside checkouts whose path contains a colon on macOS.
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/public-build"
bash fixtures/fetch.sh             # optional raw test fixtures; downloads large CC0 files
bash ci.sh                         # fmt, clippy, release workspace tests, cargo-deny
bash apps/mac/build-ffi.sh         # Rust static library and Swift bindings
(cd apps/mac && bash Support/make-app.sh)  # builds apps/mac/build/Tessera.app
```

`ci.sh` can take a while; the [Mac app build guide](apps/mac/README.md) also covers Swift tests, Xcode, release builds and signing. On a colon-containing checkout, keep `CARGO_TARGET_DIR` outside the repo for every Cargo command; do not commit `target/`. Raw fixtures live under ignored `fixtures/raw/`; without them some integration tests skip. Model weights are separate downloads on first explicit use, not part of `cargo build` or the fixture script.

## Command line and MCP

```sh
cargo run -p tessera-cli -- index /path/to/photos
cargo run -p tessera-mcp -- --app-dir /path/to/tessera-data
```

The first indexes a folder via the `tessera` CLI; the second starts the `tessera-mcp` JSON-RPC server on stdio (connect it as an MCP subprocess, not as an interactive shell). Other CLI commands include `ls`, `cull`, `develop`, `render`, `export`, `import lrcat` and `ml`. The CLI's `mcp` subcommand requires the `tessera-mcp` binary beside `tessera` or on `PATH`. See the [MCP guide](crates/tessera-mcp/README.md).

## Documentation and contributing

Start with the [documentation index](docs/README.md), [implemented architecture](docs/ARCHITECTURE.md), [build status](docs/STATUS.md), [stack explainer](docs/12-stack-explainer.md) and [Mac app guide](apps/mac/README.md). The product specs describe targets, not necessarily implemented features.

Tessera is licensed under Apache-2.0. Contributions use the Developer Certificate of Origin (DCO), not a CLA; see [CONTRIBUTING.md](CONTRIBUTING.md) and the [licensing decision](docs/13-licensing.md).

## Releases

The macOS app uses Sparkle 2 for update checks when configured. Tagged `vMAJOR.MINOR.PATCH` builds run the release pipeline, producing a DMG, appcast and eligible deltas. See [release setup and verification](apps/mac/Support/release/README.md) for signing, notarization and the Sparkle key. Local builds default to ad-hoc signing; an empty Sparkle public key must be configured before publishing updates. Never commit private keys.
