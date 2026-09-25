# M3-05 retry verification

The required command was executed again in this worktree:

    cargo test -p ml-enhance -p pipeline-cpu -p image-core -p export --release && cargo clippy -p ml-enhance -p pipeline-cpu -p image-core -p export --all-targets -- -D warnings && cargo fmt --check

Exit status: 0. Parsed harness totals: 156 passed, 0 failed, 3 ignored.
Full output: retry-validation.log. CARGO_TARGET_DIR remained
/Users/rutmehta/.cache/tessera-target/M3-05. No worktree target directory exists.
`git diff --check` also passed. No source changes were made in this retry.

This gate does not establish work-package completion. The existing implementation
and HANDOFF.md explicitly identify missing denoise inference, PSNR acceptance,
pipeline/cache integration and fp16 validation. These are implementation gaps,
not compiler failures. The LibRaw warnings do not fail the requested gate.

A concrete scope blocker remains: the actual CLI argument parser is
apps/tessera-cli/src/export.rs, outside the allowed paths. Implementing the
requested --upscale option there requires permission to expand the allowlist.
Adding a parser to an unrelated tools binary would not wire the application's
export command. No forbidden files were modified.

Engine-api StageId::Denoise is explicitly raw-domain and precedes Demosaic.
The requested post-demosaic neural path needs an explicit placement/domain
contract rather than silently changing existing CFA recipe semantics. Proposed
engine-api requirements are recorded in crates/ml-enhance/README.md.

No kanban task id was supplied in the environment, so the initial kanban_show
returned a missing-task-id error. No unrelated board card was changed.

RESULT: FAIL M3-05 remains incomplete; denoise inference and pipeline/cache integration are missing, and CLI wiring requires expanded path permission.
