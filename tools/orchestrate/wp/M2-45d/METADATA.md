# M2-45d native metadata checkpoint

Overall result: **FAIL / incomplete**. The native metadata slice is implemented
and verified. ISO 21496-1 gain-map JPEG remains undelivered; a green regression
gate is not full acceptance of this work package.

## Delivered

- Portable native EXIF/IPTC extraction and filtering alongside XMP in JPEG,
  TIFF, PNG, AVIF, JXL and developed DNG. Source pixel offsets, previews,
  orientation, opaque MakerNotes and raw-development structures are not copied.
- All, copyright, copyright/contact, all-except-camera and none policies;
  independent person/location removal and hierarchical keyword controls.
- Native IPTC keywords are reconciled with XMP. Embedded person identities
  remain filtering context when a sidecar is supplied. Partial sidecars override
  individual properties without discarding embedded rights/contact/hierarchy.
- JPEG APP1/APP13, TIFF/DNG relocated IFDs, PNG eXIf, associated AVIF Exif items,
  JXL Exif boxes and existing XMP carriers. Native bytes count toward JPEG limits
  and are carried in PQ/HLG exports.
- CLI, FFI and MCP read actual per-image source metadata. MCP exposes the policy
  and privacy controls; original-copy mode still preserves bytes and rejects
  reduction/privacy options. Swift binding generation has no tracked diff because
  FFI options remain JSON.
- Independent ExifTool fixtures/readback cover 240 policy/privacy/hierarchy
  combinations, all-format input roundtrips, endian differences, host batches,
  malformed inputs and compressed metadata budgets.

The detailed implementation and source-format boundaries are in
`METADATA-PROGRESS.md`. In particular, non-TIFF proprietary RAW native metadata
(e.g. RAF/CR3) is not extracted. BigTIFF, JPEG Extended XMP and compressed JXL
metadata are rejected rather than pretending to preserve/filter them. This is
not universal byte-for-byte source metadata preservation.

## Review and corrective regressions

Independent review found a misplaced-IFD-pointer panic, silently ignored Extended
XMP, and loss of embedded fields under partial sidecars. Targeted tests failed
before fixes and passed afterward. A follow-up review found the 4096-segment JPEG
scan limit could silently omit later privacy metadata; `native-segment-limit-red.log`
reproduces this, and `native-segment-limit-green.log` verifies the fail-closed fix.
The final independent review passed with no remaining reported logic/security
issues (`native-review-final.json`). Added-source security-pattern scan and source
`git diff --check` passed. Verbatim tool logs retain emitted whitespace.

## Parent-run full verification

The exact required command was executed in this worktree with
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d`:

```
cargo test -p export -p tessera-ffi -p tessera-cli -p tessera-mcp -p sidecar --release && cargo clippy -p export -p tessera-ffi -p tessera-cli -p tessera-mcp -p sidecar --all-targets -- -D warnings && cargo fmt --check && cargo deny check licenses && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build)
```

`native-verified-gate.log` ends with **GATE_EXIT=0**. Parsed test totals:
**552 passed, 0 failed, 18 ignored**. The segment-limit regression is present in
that full run. Clippy, formatting, licensing, workspace check, binding regeneration
and Swift build all passed. Swift build took 8.81 seconds. Existing native LibRaw
warnings and the blake3_neon deployment-target warning remain.

The earlier `native-full-gate.log` also passed, but was compiled before the final
segment-limit regression was added. It is superseded by the verified gate, not
used to imply that the late test ran in that earlier invocation. No code was
modified during the final verified gate. All changed repository paths match the
user allowlist; no target directory is tracked.

## Still missing

ISO 21496-1 gain-map JPEG, its host controls and verified interoperability. The
prototype passed independent MPF extraction/numerical reconstruction but failed
ImageIO pixel decoding in the worker environment. The prototype was removed.
ImageIO's independent reference-encoding attempt also failed, so the investigation
does not establish whether the blocker is platform/environment or encoder
interoperability. The failed diagnostics are retained; no working gain-map output
or Apple/Adobe interoperability is claimed. The previously committed DNG and
PQ/HLG slices remain covered by the new full gate.
