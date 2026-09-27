# M2-45c partial checkpoint: XMP export policy

Latest retry: see `ORIGINAL.md` for the additional original + XMP copy slice,
CLI/FFI integration, and passing full gate (389 passed, 0 failed, 16 ignored).
No numbered M2-45c item is complete. The notes below describe the inherited
XMP-policy checkpoint and its earlier verification history.

This is not completion of any of the three numbered M2-45c items. Native
EXIF/IPTC carriers, the DNG enhancements and HDR output remain outstanding.
The implementation here is the XMP portion of item 1.

## Do not merge: acceptance items still incomplete

No numbered item is green and no implementation commit has been made. The
previous review found a person-info leak: `keyword_value` did not resolve an
explicit `rdf:Description` resource wrapper. With PersonInImage declaring Alice,
this keyword survived remove_person_info:

    <rdf:li><rdf:Description><rdf:value>Alice</rdf:value>
      <f:source>Agency</f:source></rdf:Description></rdf:li>

This specific blocker is now fixed. The common value reader unwraps RDF
descriptions iteratively before reading child/attribute rdf:value, without
searching arbitrary qualifier descendants. Three regression tests were run
first and failed on actual Alice leaks (resource-wrappers-red.log), then the
entire sidecar release suite passed (resource-wrappers-green.log). Coverage
includes wrapped keywords, person declarations (bag/scalar/PersonName), and
People hierarchy paths, with child/attribute values and hierarchy on/off.
Surviving qualified keyword XML and the source packet are preserved. This
does not establish native EXIF/IPTC support or complete item 1.

## Implemented

- Export policies: all, copyright only, copyright + contact, all except camera
  and Camera Raw information, and the existing none policy.
- Opt-in XMP removal of MWG/Microsoft regions, IPTC person structures, their
  associated flat/hierarchical person keywords, GPS and IPTC location fields.
  Names are matched case-insensitively after trimming. Person keywords without
  a corresponding region/person declaration or People/Persons hierarchy cannot
  be identified semantically and are not guessed from arbitrary text.
- Lightroom hierarchy output retains existing paths or synthesizes single-level
  paths from flat keywords when no hierarchy exists. Disabling it drops only
  hierarchicalSubject, not flat keywords.
- Filtering resolves namespace URIs, including alternate prefix spellings and
  properties represented as attributes or elements. Nested camera/location
  properties are also removed. Surviving qualified keywords retain their XML.
- Selection updates no longer rewrite descriptive fields through a flattened
  metadata model, preserving language alternatives and qualified values.
- Developed DNG removes top-level Camera Raw/Tessera development instructions
  while retaining descriptive, contact and camera XMP allowed by the policy,
  rather than rebuilding a small descriptive subset and losing contact fields.
- CLI and FFI JSON settings expose these controls; no UI changes.

CLI:

    --metadata all|copyright|copyright-and-contact|all-except-camera|none
    --remove-person-info
    --remove-location
    --keywords-as-hierarchy true|false

FFI JSON:

    metadata: all|copyright|copyright_and_contact|all_except_camera|none
    remove_person_info: false (default)
    remove_location: false (default)
    keywords_as_hierarchy: true (default)

These remain JSON settings, not new UniFFI record layouts. Regeneration is still
part of the required gate. MCP retains its existing embed_metadata boolean;
no expanded policy controls have been added to its tool schema in this slice.

## Tests

- Sidecar policy tests cover arbitrary namespace prefixes, nested camera
  properties, attribute and structured person names, privacy, keyword hierarchy
  and preserving qualified keyword XML.
- Selection test preserves a French language alternative and a qualified keyword.
- Export matrix exercises four policies with privacy on/off in JPEG, PNG,
  TIFF16, developed DNG, AVIF and JPEG XL. JPEG/PNG/TIFF/DNG use decoder metadata
  APIs; AVIF/JXL inspect uncompressed XMP bytes, not an independent metadata
  parser. Export sidecars are also read back.
- FFI JSON round-trip and CLI argument-to-settings mapping tests.
- Initial missing API failures: metadata-red.log, export-metadata-red.log.
  Review reproductions: review-red.log. ffi-metadata-red.log records a build
  race (an old sidecar artifact while its API was being changed), not a valid
  FFI behavior-red result. The baseline gate was interrupted to release Cargo's
  build-directory lock; it was not a completed baseline verification.
- Full sidecar release suite passed, including eight focused policy tests after
  the two review-fix cycles. Qualified RDF values use one shared value reader
  for person declarations, hierarchy paths and keyword filtering, but the
  explicit resource-wrapper case was subsequently resolved by the retry above.
- metadata-gate.log stopped on the test's unsupported generic TIFF reader for
  LinearRaw DNG. The test now uses raw_decode::linear_dng instead.
- metadata-final-gate.log passed 380 tests (16 ignored), Clippy, formatting and
  licenses, then was stopped at the FFI-build lock to avoid two simultaneous
  binding regenerations. Its tests predate the final two privacy fixes.
- The exact requested gate against final code is metadata-verified-gate.log.
  It exited 101: 300 passed, 1 failed, 16 ignored before stopping at
  `crates/tessera-ffi/tests/understanding.rs:392`,
  `jobs_cancel_and_auto_suggest_follows_the_setting`, assertion
  `status.test_models`. That test passed on an isolated retry with the same
  package set. The cause was not investigated; the complete gate has NOT
  passed on this final code.
- remaining-verification.log records the isolated retry and separate remaining
  checks. It exited 0: isolated test, Clippy, formatting, licenses, FFI binding
  regeneration and Swift build all passed. Swift completed in 116.02 seconds.
  The linker warned that blake3_neon.o targets macOS 26.5 while the app links
  for 15.0; this was not changed. Regenerated Swift/C binding files had no
  tracked diff because the options continue to cross the boundary as JSON.

Final work-package result: FAIL. The inherited changes are staged but
uncommitted. No numbered acceptance item is complete. The specific wrapped
RDF privacy blocker is resolved, but metadata support is still XMP-only.

## Retry verification

- `resource-wrappers-red.log`: three new regressions failed on person-name leaks.
- `resource-wrappers-green.log`: all 53 sidecar release tests passed after the fix.
- A read-only review of the wrapper fix and its tests reported no concrete
  defects. The reviewer did not run tests or review native metadata support.
- `retry-gate.log`: the foreground tool timed out after 420 seconds during
  compilation. This was not a completed gate.
- `retry-complete-gate.log`: full gate stopped with 184 passed, 1 failed,
  10 ignored. `export_batch_does_not_starve_slider_drag` failed at
  `crates/tessera-ffi/tests/develop.rs:1198`: 93 of 120 frames were L2, below
  its 90% threshold. No performance assertion or production scheduling code
  was changed. Its isolated retry passed.
- `retry-remaining-checks.log`: isolated retry, Clippy, formatting, licenses,
  FFI regeneration and Swift build all passed (exit 0).
- `retry-final-gate.log`: the exact complete requested gate subsequently
  passed (exit 0): 380 Rust tests passed, 0 failed, 16 ignored; Clippy,
  formatting, licenses, regenerated bindings and Swift build passed.
  Swift completed in 20.30 seconds. The pre-existing macOS deployment-target
  warning for blake3_neon.o remains. Regenerated bindings have no tracked diff.
- CARGO_TARGET_DIR remained `/Volumes/betterSSD/tessera-cache/target/M2-45c`.
  No numbered item is being committed or presented as complete based only
  on the passing gate. All remaining acceptance work below is unchanged.

## Remaining acceptance criteria

1. Native EXIF and IPTC extraction/preservation/projection and filtering, JPEG
   APP1/APP13, TIFF/DNG metadata tags, PNG eXIf, AVIF/JXL Exif carriers. Independent
   carrier-aware read-back per policy and format. Expanded MCP controls if
   exposed. This checkpoint must not be presented as complete metadata support.
2. Original + XMP copy/merge mode is now covered by the partial `ORIGINAL.md`
   slice. Still required: OriginalRawFileData embedding; DNG 1.6 tag
   audit; independent LibRaw RGB comparison to the developed engine render;
   corresponding host options and CLI flags.
3. PNG16 cICP and AVIF10/12 PQ/HLG Rec.2020 with recipe HDR peak/headroom;
   ISO 21496-1 gain-map JPEG including MPF/XMP interoperability; independent HDR
   reconstruction, ImageIO decode tests, and host options/CLI flags.

No codec or licensing dependency changed.
