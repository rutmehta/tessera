# Develop save retry: source and validation record

Scope: Stage A only. A failed Develop save after the recipe rename retains an explicit repair action. A later `flush()` without a new edit rebuilds XMP/index from the current disk recipe, without republishing the old session recipe. Matching session/disk Develop state may publish its saved callback and requested edited preview; a foreign Develop replacement does neither. This does **not** add a Develop lease, Swift close recovery, external-process CAS, or a multi-file transaction.

All commands ran in `/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera` with `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, and external `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target`; subprocess gates had bounded 120- or 600-second timeouts. Each directory retains command, full stdout/stderr, direct exit, source snapshot, and SHA-256 manifest where captured. The `gate-85de2860/source-manifest-sha256.txt` has 113 relative paths and was checked again after the gate.

| Stage | Exact source HEAD | Outcome |
| --- | --- | --- |
| `red1` | `df1e8ece` | Exit 101, test fixture fault path did not match Engine's resolved path; first flush unexpectedly succeeded. |
| `red2` | `f79d18d4` | Exit 101, intended RED: first flush failed after recipe rename; idle second flush reported success but XMP was absent. |
| `red3` | `3d2f9900` | Exit 101, added controls exposed a test-only global fault-slot collision under parallel execution. |
| `red4` | `fda26a7d` | Exit 101, intended RED: all three retry/foreign-state cases failed under path-isolated faults; 16 existing Develop tests passed. |
| `gate-c00b9533` | `c00b9533` | Exit 101, preview fixture used JPEG's bypass path rather than registering a requested tier; recursive index included generated support previews; blocked-worker test timed out and had unsafe panic cleanup. Failures retained in full. |
| `gate-8dde78e0` | `8dde78e0` | Isolated exit 101 diagnosed the blocked-worker fixture: `engine.open_develop_session` consumed its sole Engine Arc, so first flush returned `engine closed` before the post-recipe boundary. |
| `gate-64ac798c` | `64ac798c` | Two-waiter 1/0, focused Develop 21/0, adjacent JPEG 1/0 and API recipe/history 1/0, formatting exit 0. Strict Clippy exit 101 on the large inline `SaveFailure` recipe payload. |
| `gate-85de2860` | `85de2860b25e658da1aa2506dafd312675270bf0` | Focused Develop 21/0, adjacent JPEG 1/0, adjacent API recipe/history 1/0, `cargo fmt --all -- --check` exit 0, `cargo clippy -p tessera-ffi --all-targets -- -D warnings` exit 0. |

The final tests use tiny generated JPEGs. The preview test calls the internal request path to register a tier because public JPEG preview reads bypass request tracking; it verifies no edited preview before repair and a stored preview plus exact saved hash afterward. The foreign-edit test asserts unchanged foreign recipe bytes, XMP settings and the index for the original image ID, and no stale session preview or saved callback. The two-waiter test holds the real writer after recipe publication and requires both concurrent flushes to report that failed attempt before a later explicit repair succeeds. The existing adjacent JPEG test exercises a real surface/undo/save/reopen path; RAW fixture and broad GPU tests were not run here.
