# M3-10 implementation handoff

Implemented the `agent` workspace crate and `tessera agent edit` CLI within the
allowed paths. No engine-api changes and no commits.

Delivered:
- Typed planner trait and Anthropic Messages, OpenAI Responses, local Ollama and
  scripted FakePlanner implementations with offline wire tests and rustls HTTP.
- Metadata/perception packets, EXIF, optional host-supplied identity/depth/caption/
  embedding cache, style priors and provider-native low-resolution attachments.
- Console-backed, reversible agent history and rationales, per-step render,
  bounded objective/VLM critique, fail-closed tool and redo constraints, and
  per-step recipe provenance surviving later failures.
- Style-profile scene/person consensus for batch edits and confidence review queue.
- Explicit offline fallback, provider/model selection, dry-run, redo, iteration
  and time-budget CLI flags.

Verification (executed, not inferred):

    cargo test -p agent -p tessera-cli --release && cargo clippy -p agent -p tessera-cli --all-targets -- -D warnings && cargo fmt --check

Exit 0. 57 tests passed, none failed or ignored. Full output: `verification.log`.
`git diff --check` passed. A programmatic changed-path check found no out-of-scope
files. CARGO_TARGET_DIR remained `/Users/rutmehta/.cache/tessera-target/M3-10`.
Vendored LibRaw emits existing C compiler warnings; Rust clippy -D warnings passed.
No live/paid provider requests were made. HTTP/TLS dependency licenses were checked
from locked Cargo metadata and recorded in `crates/agent/README.md`.

Integration boundaries and needed engine fields are explicit in that README:
host-provided persistent identities/depth/nearest-caption, geometry-aware face-box
transforms, and a safe typed tool for learned settings outside basic tone/WB.
The time budget is cooperative for synchronous rendering and enforced as an HTTP
timeout. Global WB redo preserves sky masks/other controls, not sky pixels.
Provenance is a recipe extension, not a signed C2PA credential. Review UI group
amount/toggle support remains upstream UI/API work.
