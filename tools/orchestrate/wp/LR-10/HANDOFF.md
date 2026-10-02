# LR-10: integration checkpoint

## Status

Rendering implementation is paused at the user's no-golden-change boundary.
No LR-10 rendering code, private pixels, source identities, or private content
hashes have been committed. No Develop pixel golden has been modified.

## Integration rebase

Rebased `wp/LR-8-smart-preview-proxies` from `840e2c88` onto
`origin/wp/LR-9-default-noise` (`46b1bf54`), preserving the commit sequence.

Conflicts:

- `crates/tessera-ffi/src/lrcat_profile.rs`: retained the integrated LR-9
  audit keys, value classifications and no-op policy accounting rather than
  replacing them with the earlier B5-51 harness.
- `crates/tessera-ffi/src/lrcat.rs`: retained depth and combined resolver tests;
  retained combined mask/depth application and rollback, adding LR-8's
  orientation-adjusted extent. OfflineProxy remains an admitted outcome.
- `crates/pipeline-cpu/src/lib.rs`: unioned local-adjustment hook exports with
  the existing context-aware render export.
- `crates/import-lrcat/tests/golden.rs`: retained the upstream expected digest
  pending the explicit no-golden-change clarification below.

Integration compilation also found an LR-3 synthetic metadata initializer
missing LR-8's `catalog_orientation` and `baseline_exposure`. Neutral defaults
were added. This changes no rendering behavior.

The schema predicate union and LR-9 no-op table are unchanged from the rebase
target. LR-7's single Import history recording is retained. The only Cargo.lock
changes against the target are the two approved raw-decode edges: jxl-oxide and
zune-jpeg 0.5.15. No board.json change.

## Golden boundary

The import-lrcat golden target fails before any LR-10 implementation:

- `synthetic_catalog_output_including_retained_source`: LR-8 retains the
  catalog orientation in recipe metadata, changing serialized catalog output.
- `lr6f_active_blur_and_inactive_depth_catalog_golden`: the structural difference
  at its first failing row is exactly `/recipe/lightroom_orientation` present
  in the integrated result but absent in the upstream retained baseline.

These are import serialization checks, not Develop pixel goldens. No expected
value or fixture has been changed. The user was asked whether these integration
baselines may be reconciled while keeping every Develop pixel golden fixed.

## Verification

- Workspace Clippy (`--workspace --locked --all-targets -- -D warnings`): PASS.
- `cargo fmt --all -- --check`: PASS.
- UniFFI regeneration: PASS; only pre-existing generated Swift whitespace differs.
- Existing native RAW Develop goldens (`raw_fixture_goldens`): PASS, byte-identical.
- Full requested release Rust run (`--locked --no-fail-fast`, no command-level
  exclusions): 2,026 passed, two failed, 69 suite-declared ignored. Both assertion
  failures are the import serialization checks listed above. Initial compilation
  found the LR-3 metadata initializer and was retried after the neutral fix.
- The run's doctest phase also encountered E0460/E0463 dependency artifact errors
  after overlapping binding generation rebuilt dependencies with macOS 15.0.
  A serial retry of all 13 requested crates' doctest targets with a consistent
  deployment target passed. The full Rust gate still fails the two import goldens.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**; 920 XCTest cases, three
  skipped, zero failures; five Swift Testing cases passed.
- `swift build -c release --product Tessera -Xswiftc
  -strict-concurrency=complete -Xswiftc -warnings-as-errors`: PASS (152.00 s).
  The existing linker warning about the BLAKE3 object targeting macOS 26.2
  rather than 15.0 remains; no installed-app or macOS 15 runtime check was run.

LR-10 tests, implementation and the 12-pair before/after measurement have not
been performed. No parity improvement is claimed. No app was installed or
launched; no source bundle was written.

## Engine ownership

No LR-10 engine implementation was started. Integration reconciliation touched
`crates/pipeline-cpu/src/lib.rs` (union of public exports and formatting),
`crates/tessera-ffi/src/lrcat.rs` (combined resource hooks and orientation),
`crates/tessera-ffi/src/lrcat_profile.rs` (retain upstream aggregate audit), and
`crates/tessera-ffi/tests/lr3_develop_retouch.rs` (neutral synthetic metadata
fields). These corrections are in the existing rebased commits; this lane's
only additional commit is the handoff. All pre-existing LR-8 engine changes
remain listed in `../LR-8/HANDOFF.md` and `../LR-8/LR-8b-HANDOFF.md`.

## Public sources located for the pending implementation

- [DNG Specification 1.7.1](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/DNG_Spec_1_7_1_0.pdf):
  ProfileToneCurve (p. 51), DefaultBlackRender (p. 62), exposure offset and HSV
  encoding (pp. 63-65), camera transform and tables (pp. 100-104).
- [Public DNG SDK dng_render.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_render.cpp):
  `dng_tone_curve_acr3_default::Evaluate`, exposure ramp, default shadows value,
  and DefaultBlackRender handling. No SDK values or binary profiles have yet
  been added to the implementation.

The specification leaves Auto black subtraction method/amount reader-dependent.
The public SDK sample ramp could be implemented explicitly as an approximation;
this checkpoint does not claim Lightroom's image-dependent black heuristic.


## Local checkout state

Rebased integration tip before this handoff: `d586055b`. The pre-existing
uncommitted generated Swift whitespace was preserved and restored after rebase.
No push, board update, foreground GUI launch, or application installation occurred.
