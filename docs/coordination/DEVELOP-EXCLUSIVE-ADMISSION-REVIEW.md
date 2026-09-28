# Exclusive Develop admission — integration review

2026-09-28 11:32 UTC. Machine A integration review of validated source `593730668d6bcc5f65b8ea88b2febc999393e1f1` and evidence package `3cb0c24902902bda3e7abd68b08ce14c32eb533a`.

A second participating editor now fails before creating a save worker, and direct recipe replacement fails while the destination has an active Develop owner. Selection and read-only saved-recipe histograms remain available. Admission and the initial recipe/baseline snapshot share the existing destination gate. Saves and repair authenticate the original gate identity, owner ID and current destination.

The session and save worker hold owning reservations; render state holds only non-owning authority. Successful close releases after writer drain, failed close retains ownership, and final session destruction on the writer thread retains ownership through actual worker exit. Failed snapshot/decode opens release their reservation. Owner IDs fail closed on exhaustion. No public ABI or Swift product change.

## Verification

- Tests-first checkpoint d61512ed reproduced both intended admission failures (direct101); foreign-disk baseline control passed.
- Final checkpoint59373066 passed all137 FFI library tests, eight focused tests and ten recipe-gate tests, formatting and strict Clippy. The selected groups overlap the full suite:155 invocations,137 unique tests.
- Root and Astra inspected raw outcomes and frozen source records. All12 final groups have unchanged before/after1177-input manifests. Astra checked those inputs against Git. Root confirmed the four integrated Rust files are byte-identical to59373066 and packaging3cb0c249 changes no Rust/Cargo source.
- Root independently verified all472 evidence payloads against committed SHA-256 entries and copied bytes. Manifest: `tools/orchestrate/wp/UX-03/evidence/develop-exclusive-admission-tests-2026-09-28/relative-SHA256SUMS.txt`.
- UI rejection/re-entry and stale-error controls were integrated separately in049bfe95:34 unique adjacent tests passed on the accepted4a45 archive. This is not an updated app/archive or new GUI acceptance claim.

The compile-only4f26 missing-import failure,096 strict-lint failure, zero-selection attempt, and all raw warnings remain preserved. A move/use issue at592 was caught before execution. None count as passing gates.

## Limits

Existing keys canonicalize the parent but retain the exact recipe filename. Case-variant aliases on case-insensitive filesystems remain outside this guarantee; see DEVELOP-DESTINATION-ALIAS-LIMITATION.md. External/Agent/Cull/import writers, filesystem CAS, global Quit and all-writer durability are not covered. Existing OwnerBaseline defense remains. No preview rebuild or activation was performed.
