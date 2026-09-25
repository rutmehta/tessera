# WP M0-01 — Cargo workspace scaffold, CI, licence gate, fixtures script

Create, at the repo root:
- `rust-toolchain.toml` pinned to `channel = "stable"` with `components = ["clippy","rustfmt"]`. Run `rustup update stable` if the installed stable is < 1.89 (ort needs 1.88, lcms2 1.89).
- `Cargo.toml` workspace (resolver 2) with members: `crates/image-core`, `crates/raw-decode`, `crates/pipeline-cpu`, `crates/pipeline-gpu`, `crates/recipe`, `crates/sidecar`, `crates/index`, `crates/previews`, `crates/import-lrcat`, `crates/cull`, `crates/ml-runtime`, `crates/jobs`, `crates/engine-api`, `crates/compositor`, `apps/pe-cli`. Each crate is a minimal lib (pe-cli a bin) with a doc comment naming its purpose (see docs/11-execution-plan.md §3) and one trivial test. Shared `[workspace.dependencies]` with pinned versions: `wgpu = "30"`, `rusqlite = { version = "0.40", features = ["bundled"] }` (add `"fts5"`/`"rtree"` features if available in this version, verify), `serde`, `serde_json`, `thiserror`, `anyhow`, `tracing`, `bytemuck`, `half`, `zune-jpeg = "0.5"`, `jxl-oxide = "0.12"`, `lcms2 = "6"`, `ort = "2.0.0-rc.13"`, `uniffi = "0.32"`. Only `image-core` and `pe-cli` need actual deps wired now; the rest just declare. Everything must compile.
- `deny.toml` for `cargo-deny` (install it with `cargo install cargo-deny --locked` if missing): allow MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0, CDDL-1.0, LGPL-2.1 (warn); deny GPL-3.0, AGPL-3.0. Ban crates `jpegxl-rs`, `jpegxl-sys`, `libraw-sys`.
- `ci.sh`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check licenses bans`. Must pass.
- `fixtures/fetch.sh`: downloads a small CC0 sample set from https://raw.pixls.us (one file each of Canon CR3, Sony ARW, Nikon NEF, Fujifilm RAF, and a DNG; pick real URLs from the site's index, verify with curl -I) into `fixtures/raw/`, skipping existing files; `fixtures/` is git-ignored except `fixtures/fetch.sh` and `fixtures/golden/`. Actually run it once and confirm the files download and are > 5 MB each.
- Update `.gitignore`: `target/`, `.worktrees/`, `fixtures/raw/`, `.DS_Store`, `*.xcuserstate`.
- `README.md` at root: 15 lines, what this is, how to build, link to docs/.

Test command: `bash ci.sh`.
