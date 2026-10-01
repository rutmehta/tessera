# LR-0 handoff: source inventory awaiting coordinator review

Base: `68264c74c365d8e7a4c33ddaecebf934f9305b08`.
Branch: `codex/lr-0-inventory`. Source-only documentation; no product implementation.

Read `docs/coordination/LR-TRANSLATION-MATRIX.md` first. All 199 explicit Lua mappings were compared with all 143 CrsKey declarations: zero missing CrsKey names and 56 unsupported passthrough names, each present in the matrix. Parent independently reproduced these counts with a source-text check; `INVENTORY-CHECK.json` records the exact names. This is not a runtime test or Adobe compatibility proof.

Astra authored the inventory; Luna independently scouted test design and integration gaps, then reviewed the final source inventory: no blocking corrections; one source-path clarification applied. This is internal source review, not the required Claude coordinator approval. Supporting plans and public schema research are in this directory. Adobe-hosted community examples are clearly non-normative observations, not specifications. No private catalog content was inspected or committed.

RED/GREEN, Rust tests, clippy, fmt and CPU render gates: **not run / not applicable to this documentation-only LR-0**. No lane implementation is declared passing. Review this matrix before LR-1+ implementation as required by the brief. Request decisions on representation changes, unsupported/cloud-only handling and sources of authoritative format evidence; do not infer full fidelity from mapped fields.

Key findings: CPU rejects Point Color; recipe retouch is not consumed by full develop rendering; local defringe/color overlay are not implemented; source mask rasters need durable asset references beyond a regenerable cache; stored Upright matrices and several legacy/HDR settings need representation decisions. Existing render/inference seams support synthetic test doubles but do not establish parity with Adobe.

Machine B: B5-29c performance ownership remains yours. This branch edits only documentation, no `lua_develop.rs`, `xmp.rs`, codec, recipe or renderer source. Future additive mapping hunks will be coordinated after the inventory review. Claude A retains independent review, Swift gates and all main merges.

Accepted assignment publication: `928e9f33-242f-424c-bdca-8f2fff607951`. Peer receipt remains unverified. Managed checkout was moved across volumes using `mv` and `git worktree repair` after Git's native move refused a cross-device link; its live path is `/Volumes/betterSSD/tessera-worktrees/codex-lr-0-inventory`. No previous worktree was reset or cleaned.
