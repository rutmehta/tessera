# Agent batch fix source review

Reviewed `/tmp/tessera-agent-batch-fix.patch` source only. No builds/shared edits.

## P1 — Parent-directory resync still reads the excluded reappeared target

The new fixed availability set prevents direct edit/report reads, but successful available-image `finish` still calls `Engine::resync` in agent_runs.rs453–466. That performs `c.index.scan(path.parent(), &catalog::Sidecars, ...)` over all siblings. The new test listener recreates the missing sibling with changed mtime and an invalid recipe. Index::scan checks changed metadata/sidecar stamp, then calls `sidecars.read(path)?` at index/src/lib.rs260; catalog::Sidecars reads the invalid recipe at catalog.rs55. This propagates an error from the available photo's finish and aborts the report (or coherent batch reports an error), defeating both the promised no-read rule and partial success. Restrict catalog refresh to the admitted file or carry an immutable exclusion for unavailable target paths through scanning; do not remove the reconnect-with-invalid-recipe test.

Other scoped observations: all ID reservations and dirty journal checks precede availability/destination admission; generic acquire remains strict. Missing targets are not authorized writers and caller-order availability is fixed. Sequential edits skip them, coherent inputs retain explicit original-index mapping, and direct final report construction avoids reading their sidecars. Added mixed independent/coherent and dirty/active-editor tests appropriately exercise intended behavior. Approval pending indirect scan correction and runtime gates.

## Revised single-file resync disposition

Reviewed revised patch: blocker resolved by Index::scan_file and agent resync using it. Shared scan_scope preserves existing folder-scan canonical root, entry filters, metadata/sidecar transactions and change notifications; scan_file uses the admitted canonical file as WalkDir root, so sibling reappearance cannot enter its read path. Generic original-write reservation remains strict. The missing target remains outside destination authorization and report sidecar reads. No remaining actionable source defect found. Source-approved pending focused native and existing index regression gates; no builds run here.

Revised patch SHA-256: `5094ecc55aa404374b673a959b3225525c610b5afa3ab9c03c3938640bdde22f`.

## Public Index wrapper follow-up

Reviewed added `crates/index/src/api.rs::Index::scan_file` delegation, which was missing from the first candidate. It forwards identical path/readers to the already reviewed Core method and maps the error type using the same convention as scan. No additional policy or I/O behavior; source-approved. Prior source review did not catch this facade omission, and compile23 appropriately exposed it. Compile24 is owned by implementer and not claimed as passing here.

Final reviewed four-file patch SHA-256: `8f7a3fc3a1d8ea88c305334df1293e08fb27b94312611122dd4f966fa63eb634`.

## Agent perception Console follow-up

Runtime24 exposed a second sibling read missed in the earlier scoped review: Agent::perceive used Console::open_image, which scanned the whole parent into its private index. Reviewed new open_image_only/open_image_scope path and Agent::perceive call change. Existing public Console::open_image still requests the original whole-folder scan; new agent-only path delegates to scan_file before the unchanged indexed-path lookup and perception/render reads of that same target. No actionable defect found in this added delta. Agent batch inputs remain fixed admitted targets. Expanded agent/Console/index regression gates are appropriate; source acceptance is not runtime success.

Revised six-file candidate SHA-256: `db4c3a4c118f499f2e0f89b6d6d267c99a882eafa7046c4d0daaef9bd1557e53`.
