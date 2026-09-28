# RAW capture Task 1 ownership checkpoint

Candidate: 01409e822b19a45f68e81a8eb16f0ef4fbc3f166, codex/private-raw-capture, base838c8e496bf4cb5bcecf74801e7402bedababfcf. Worktree clean after commit. No merge, archive regeneration, UI or GPU activity.

- 01-compile: exit101; expected absent private scaffold methods plus actual generic filesystem callback lifetime error. Preserved compile-only failure; not behavioral RED.
- 02-behavior-red: exit101; tests compiled, 0passed/12failed against explicit Unsupported ownership scaffold. Missing ownership was the observed failure; this does not claim twelve independent mutation tests.
- 03-ownership-green: exit0;12 ownership tests passed.
- 04-full-raw-decode: exit0;27unit+3integration passed,0ignored,0doctests. Existing fixture test executed.
- 05-strict: exit0; raw-decode Release all-targets Clippy -Dwarnings. Existing vendored LibRaw C++ warnings retained in log; no claim native compiler emitted no warnings.
- 06-fmt: exit0; workspace fmt check.
- 07-final-raw-decode: exit0 at committed candidate;27unit+3integration passed,0ignored,0doctests.
- git diff --check exit0; committed five owned files only. Cargo normal lock update adds existing raw-decode dependencies; no version changes.

Every cargo command used explicit shared BetterSSD target, MACOSX_DEPLOYMENT_TARGET15.0 and buildjobs2. Commands, direct exits and full tracked/new source hashes retained; run02 through07 before/after maps equal. Run01 has initial manifest; its normal Cargo.lock dependency update is expected. Existing RAW fixture test follows checkout fixtures/raw symlink to main fixture directory. Five files (DNG/RAF/ARW/NEF/CR3) were hashed after04 and unchanged after07; no before04 hash claim. They were read by existing regression tests, not the new ownership API. No user original was edited.

Task1 reserves full max-asset allowance plus a slot, retains pool through stage ownership, releases only after absence, quarantines failed unlinks without Arc cycles, reports Drop errors best-effort, and performs bounded nonrecursive teardown. Twelve tests include concurrent admission, cleanup-failure paths, setup-failure paths, modes and cancellation.

This is an intermediate checkpoint only. Public capture explicitly returns Unsupported, no copies/hash/sealing or decoder consumer exists yet. Private allocate_stage has a narrow temporary dead_code allowance until Task2 uses it; no lint suppression for tested behavior. Tasks2/3 and independent Task1 review remain required before component acceptance. Compiler lane released after all commands exited.
