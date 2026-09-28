# Whole-branch review — pinned RAW descriptor

Reviewed base `5f33e174198731c607ce4fa91e73586cc4505bd4` → head `083018a967d2ba1b07570824e885baca1e9d4314` in `/Users/rutmehta/.codex/worktrees/export-integration/tessera`. The supplied whole-branch diff matches the exact Git diff (unified context 10). Read the task brief, final report, progress ledger and both prior reviews. No source changes, builds, test reruns, apps or subagents were used; this review document is the only artifact written.

## Findings and verdict

- **Critical:** none.
- **Important:** none.
- **Minor:** none requiring a change.

**Ready to merge for the stated pure descriptor scope.** All previous actionable findings are addressed in the final branch. No demonstrated authority bypass, silent settings sanitization, serialization-integrity defect or compatibility regression was found. This verdict includes the saved final formatting gate, which was available by review time.

## Source assessment

The public descriptor has private fields and no public Deserialize implementation or Recipe getter (`src/pinned_raw.rs:57–63`). Both construction paths enforce the same validation, and reopen compares the stored RecipeHash with the recomputed current-settings hash (`:157–180`). Descriptor version 1, explicit recipe schema 3, matching owner, Raw source, Native revision 2, default geometry, positive declared length and the closed route are enforced. ASCII suffix normalization is bounded and cannot admit a pathname.

Current settings are parsed separately and used directly in a temporary Recipe solely for the established hash (`:115–135`); opaque history is never parsed as History or replayed. Recursive raw-versus-typed shape comparison rejects unknown settings keys, including enum and flattened array members, while preserving documented default omission and the two supported legacy profile string representations (`:218–252`). Number-to-null detection rejects non-finite f32 conversion. Named unchanged-code inspection confirmed Recipe::new/recipe_hash semantics, ImageId parsing, and the CameraProfileRef/LensProfileRef compatibility representations; no broad unrelated source exploration was needed.

Exact original UTF-8 recipe bytes are retained in memory and in the wire string. JSON escaping of the enclosing descriptor does not normalize the embedded recipe payload. Input identity domain-separates the exact payload digest and covers declared asset digest/length, owner, route and normalized suffix; locator is excluded (`:195–214`). Recursive decoded-key duplicate detection precedes semantic parsing for both descriptor and recipe payloads (`:255–317`). RecipeHash remains the existing understood render-state checksum, distinct from full input identity and from physical or pixel verification.

Tests provide meaningful positive and negative controls for the bounded claims: nondefault current exposure against stale valid history, opaque malformed history, stored hash tampering, exact byte preservation, identity dimensions, explicit headers, unsupported versions/routes, default settings omission, recursive duplicate decoded keys, nested unknown settings, flattened masks/ModelRef fields, legacy/canonical profiles and f32 overflow of both signs and inside arrays. Existing contract-version assertion changes only 1.6.0 to 1.7.0. CONTRACTS describes an additive API revision and explicitly avoids asset/render admission claims; existing recipe and process versions remain unchanged.

## Independent saved-evidence audit

Evidence directory: `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor`.

Read raw logs, durable child return-code files, metadata and before/after snapshots for:

| Prefix | Command | Observed result |
| --- | --- | --- |
| `final-engine-tests-rerun` | `cargo test -p engine-api --release --jobs 2` | Direct child 0; 63 unit + 35 integration = 98 named passes, 0 failures; focused 20 included; 0 doc tests |
| `final-clippy-rerun2` | `cargo clippy -p engine-api --all-targets --jobs 2 -- -D warnings` | Direct child 0; completed without diagnostics |
| `final-rustfmt-check-rerun` | `rustfmt --check --edition 2021 --config skip_children=true` on the four changed Rust files | Direct child 0; empty successful log |

All three before/after pairs are equal, record exact clean head `083018a967d2ba1b07570824e885baca1e9d4314`, and have the same aggregate tracked-tree hash `abbedcb6e4b022a3b4d2b173bfb3772ddf69ed7f63377b296a56822b4bc38be2`. Independently compared all five changed-file SHA256 values to both their exact head Git blobs and the current clean worktree: zero mismatches. This is five-file Git verification plus aggregate before/after equality, not an independent recomputation of every tracked blob.

Raw log SHA256 values:

- Full engine tests: `c73ab6b5b659bd1d1f7fd6dbdfc108596ac9ec3fb38f40032ef3ae9b2c5e2353`.
- Strict Clippy: `82c67497a09ef8ccbe4499fed89b44b7991d147c830b3fa1be5fd3df58cd456b`.
- Formatting: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (empty).

Metadata identifies the assigned export-integration checkout, explicit BetterSSD Cargo target and rustc/cargo 1.98.1. Read the durable runner: it saves the child process return code independently of its own exit status, refuses reused labels, and captures source before and after. Counts above come from final raw logs; earlier focused passes are not added. Early failed drafts retain their documented incomplete untracked-source/runner provenance and are not used as final gates.

## Declined-to-judge boundaries

This review does not establish real asset existence, digest or length correctness, decoder compatibility, profile/model availability, render admission, pixel correctness, caching eligibility, writable history integrity, capture ownership, FFI/UI behavior, or source-backed Open in Layers capability. Those are deliberately outside this declaration-only branch. No claim is made that successful descriptor parsing authorizes publication of pixels.
