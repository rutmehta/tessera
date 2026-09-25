# WP M0-08 — Rename everything to Tessera

The product is now called **Tessera** (a single tile of a mosaic; the engine is built on image tiles). Apply the name everywhere, without changing behaviour:
- Root `Cargo.toml` workspace: package/description strings; `apps/pe-cli` → `apps/tessera-cli` with binary name `tessera` (update workspace members, any path deps, ci.sh, README).
- `apps/mac`: product name `Tessera`, scheme `Tessera`, bundle id `dev.tessera.app`, targets `TesseraCore`/`Tessera`/`TesseraCoreTests`, `make-app.sh` output `build/Tessera.app`, window title, README, ACCEPTANCE.md. Keep the build commands working: `swift build && swift test` and `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' build` (use a derived-data path outside the repo).
- `docs/`: replace "PhotoEditor" / "the app" placeholders and the repo name `lightroom` with Tessera where a name is used; add the one-line name origin to docs/README.md. Do not rewrite spec content.
- Root README.md: title "Tessera", one paragraph, licence line (Apache-2.0), link to docs.
- `tools/orchestrate/run-luna.sh`: CARGO_TARGET_DIR cache dir → `$HOME/.cache/tessera-target/$WP`.
Search for leftovers: `grep -rn -i "photoeditor\|pe-cli\|photo-engine" --exclude-dir=.git --exclude-dir=target --exclude-dir=.build .` must return nothing except historical WP briefs under tools/orchestrate/wp (leave those).

Test command: `bash ci.sh && (cd apps/mac && swift build && swift test) && ! grep -rn -i "photoeditor\|pe-cli\|photo-engine" --exclude-dir=.git --exclude-dir=target --exclude-dir=.build --exclude-dir=tools .`
