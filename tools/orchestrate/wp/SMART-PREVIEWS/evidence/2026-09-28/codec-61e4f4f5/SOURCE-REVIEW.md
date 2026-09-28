# Smart Preview codec source review

Reviewed the prepared codec, its integration test, the unapplied wiring/dependency patch, the September 28 specification, and existing prefix/lens/metadata implementations. No edits to implementation, builds, tests, or subprocess agents. All runtime behavior remains UNRUN.

## Actionable findings

### P2 — Validate named lens mode against the captured resolution

Location: `crates/pipeline-cpu/src/smart_preview_codec.rs:405–409`.

The consistency check only constrains Embedded and whether a sample exists. It therefore admits `lens.profile = Database { profile: ... }` with `correction.source = Manual` and no sample, or with an Image sample. Those states cannot be produced by `resolve_with`: a named database profile either supplies a Database sample or errors (`lens_resolve.rs:459–464`). A checksum-valid malformed snapshot can consequently reopen, pass `validate_prefix` for the named-profile recipe, and silently omit/use different profile geometry and vignette. Conversely, Database samples are accepted for None or AutoCalibrated, which also bypasses the generator's selection rules. Extend the mode/source compatibility checks without re-resolving the lens; preserve the legitimate Image CA-only case for None. Add authenticated malformed-snapshot cases for these combinations.

### P2 — Freeze nested prefix schema instead of inheriting recipe defaults

Location: `crates/pipeline-cpu/src/smart_preview_codec.rs:45–49,349–350`.

The outer Snapshot is strict, but its recipe settings are existing `#[serde(default)]` structs that ignore unknown members. For example, a checksum-valid snapshot with `linearize: {}` silently becomes the current default ReconstructColor, and unknown nested reconstruction/lens fields are silently dropped. CalibrationSample/BrownConrady also ignore unknown members. This undermines the separately versioned snapshot's exact prefix/schema boundary: an incomplete snapshot can acquire a prefix different from the one baked into its payload, and future nested fields are accepted without understanding them. Use strict codec DTOs (including required fields) or equivalent strict structural validation before conversion. Keep general recipe unknown-member preservation separate. Add missing nested-field and unknown nested-field cases alongside duplicate-field/version tests.

## Source checks with no additional finding

- F16 admission checks every finite sample against the stated absolute-plus-relative error bound; any failure selects whole-payload F32. Image::new rejects decoded nonfinite values. Signed/HDR data is not clamped here.
- All RawMetadata fields are captured; f64 calibration uses serde_json float_roundtrip in the proposed dependency patch. Resolved numeric lens samples/manual CA are stored, and embedded opcodes are reparsed without image estimation.
- Header framing checks happen before metadata/decompression allocation, metadata is limited to 4 MiB, raw payload to 2560²×3×4, and compressed payload has an explicit bound. Output length is exact and zstd window_log_max(23) limits the decompressor window. Opcode parsing validates count/length before associated allocations.
- Container digest covers the first 64 header bytes plus all metadata/compressed bytes, excluding only its own 32 bytes. The separate raw payload digest is checked after bounded decompression.
- Exact scalar encoding is identified by the encoding enum and dimension-derived length. Unknown top-level encoding/generator/container versions are rejected in the source. Ordinary serde struct duplicate known fields are rejected; there is no dedicated runtime duplicate regression in the submitted tests.
- Source digest and byte length are explicitly assertions, never claimed to verify photo bytes. This format is correctly identified as internal camera-linear, not DNG.
- The submitted tests cover default/custom WB render comparison, nonidentity profile data, manual CA, odd crop/scale, embedded bytes, signed/HDR/F32 fallback, corruption and size/decompression rejection. Their execution and compiler/API compatibility are unverified.

The two findings concern malformed/schema-incompatible but checksum-valid snapshots; no source-level normal encode/decode fidelity regression was found. This review does not qualify useful compression on real RAW data, persistence publication, or end-to-end offline/UI behavior.

## Scoped follow-up source review — fixes before application

Re-read the revised codec and tests, limited to the two findings and regression risk. Both original P2 findings are addressed in source; no additional actionable issue found in this scope.

- Lens mode/source compatibility now matches resolver selection: named Database requires Database, Embedded requires Embedded, AutoCalibrated admits Manual/Image, None retains the legitimate enabled-CA Image case, and Auto retains the existing embedded-priority check. The new minimum 8×8 active crop admission for Image matches the resolver's estimation gate. The internal Image-sample round-trip fixture was correspondingly expanded to 8×8, so that new guard does not invalidate its intended setup.
- Frozen nested key checks require the exact version-1 prefix and calibration fields, including explicit nullable model/sample fields and all named-profile identity fields. Typed deserialization precedes Value inspection, retaining known duplicate-member rejection rather than allowing the map pass to erase the duplicate first. The fixed key lists match the current serializer's field names and emitted shape; unsupported learned/denoise variants remain rejected.
- Re-signed negative tests exercise the previously accepted mode/source combinations, missing and future nested members, nested named-profile identity, and duplicate frame_index. The positive None+Image CA case protects the intentional resolver exception. Existing named profile serialization emits filename/digest/setup, consistent with the new required-key check.

No builds or test execution were performed. Compiler/API compatibility and all new/existing runtime tests remain pending; this is source-review closure of the two findings, not a passing-test claim.
