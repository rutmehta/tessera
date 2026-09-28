## Global Constraints

- Descriptor schema version 1; explicit raw recipe schema 3; recipe ImageId matches declared recipe owner; source kind Raw; process Native revision 2; default geometry.
- Preserve exact recipe JSON bytes through serialization/reopen. Use the existing RecipeHash definition for the understood render-state checksum; do not label it full pixel or dependency identity.
- Asset digest and positive byte length are declarations supplied by a future capture owner. Validation does not establish that a file exists or has those bytes.
- Decoder route is a closed libraw-CFA-v1 enum. A normalized extension hint is 1–16 lowercase ASCII alphanumeric characters (normalize uppercase input), never an arbitrary pathname. Locator hint never participates in identity or executes anything.
- No filesystem/decoder/renderer/session/writer/FFI/UI calls. No production file-format version change or change to Open in Layers. Unsupported inputs return an error, not sanitized settings.
- User authorized autonomous board execution and parallel agents; A owns this engine-only implementation and main integration. B reservations remain untouched.

## Review Focus

- Old/missing recipe schema must reject before normalization; test raw JSON omissions and versions 2/4.
- Mismatched recipe owner must not be repaired by constructing a new Recipe; test same payload against another ImageId.
- Exact bytes and unknown top-level fields must survive roundtrip; validation is not render admission for unknown settings/dependencies.
- Current settings alone are authoritative. History is retained opaque and never replayed or validated as a writable recipe. A history/current-settings disagreement must not replace current settings. No public Recipe getter.
- Changes to path hints must not change declared captured-input identity; identity includes asset digest, length, recipe owner, exact recipe payload digest, decoder route and canonical suffix. Exact payload whitespace/history/unknown metadata changes therefore change input identity even if RecipeHash is unchanged. Identity is not filesystem verification or pixel-cache eligibility.

## Coordinator rulings

- Current-settings snapshot semantics replace the initial whole-history invariant; preserving opaque history avoids silently choosing another state. Cost if wrong: a future writable-recipe consumer needs a separate history validator.
- Reject duplicate decoded JSON keys throughout both payloads and unknown current-settings members recursively; preserve opaque top-level metadata/history. Cost: future settings require explicit compatibility work rather than silent acceptance.
- Exact recipe bytes participate in input identity, alongside owner/asset metadata/route/suffix; locator does not. Cost: semantically equivalent whitespace/history variants have distinct input identities. This is not pixel-cache identity.

## Task 1: Pure descriptor boundary and validation

**Files:** create `crates/engine-api/src/pinned_raw.rs`; expose module in `crates/engine-api/src/lib.rs`; create `crates/engine-api/tests/pinned_raw.rs`; document bounded invariant in `crates/engine-api/CONTRACTS.md`; update the existing contract-version assertion only in `crates/engine-api/tests/m532_channels.rs`. Additive CONTRACT_VERSION moves from 1.6.0 to 1.7.0 per crate policy; existing recipe schema/process/hash semantics remain unchanged.

**Interfaces:** `PinnedRawDescriptor::new(input: PinnedRawInput) -> EngineResult<Self>`, `PinnedRawDescriptor::from_json(bytes: &[u8]) -> EngineResult<Self>`, `to_json(&self) -> EngineResult<Vec<u8>>`, read-only `recipe_json(&self) -> &[u8]`, `input_identity(&self) -> Digest`. `PinnedRawInput` contains declared asset Digest/byte length, recipe ImageId, exact JSON bytes, decoder route, normalized suffix, optional locator hint. Descriptor fields remain private; do not derive public Deserialize that bypasses validation. Use explicit descriptor version in private wire representation and store a RecipeHash verified on reopen.

- [ ] Write integration tests with a valid recipe created using `Recipe::new(ImageId(1)).to_json()`. The new descriptor roundtrip must preserve `recipe_json()` bytes exactly and retain input identity.
- [ ] Add explicit rejection cases: missing/schema2/schema4 raw recipe header, mismatched owner, RGB source, Adobe/native unsupported revision, nondefault geometry, missing source/process/settings headers, duplicate decoded object keys at any JSON depth, unknown current-settings keys recursively, zero declared byte length, invalid suffix/path characters, unknown descriptor version/decoder route, and altered stored RecipeHash.
- [ ] Establish a behaviorally meaningful RED where feasible with a permissive draft boundary; distinguish any missing-module compile failure from an executed failing assertion. Preserve the raw command/exit/source. Do not merge a permissive draft.
- [ ] Implement minimal pure validation. Reject recursive duplicate decoded JSON keys before raw Value/header inspection. Require explicit schema, owner, source kind, process family/revision and settings. Parse typed current settings only; do not parse/replay history to select them. Reject unknown current-settings keys recursively (including nested arrays/enum payloads), while accepting omitted documented default settings. Require default geometry and supported metadata. Do not call Recipe::validate or imply full history validation. Preserve original bytes; never serialize normalized Recipe over the supplied snapshot.
- [ ] Add byte-preservation and identity controls: whitespace/unknown top-level metadata roundtrip; locator-only change has same input identity; changed asset digest or valid recipe settings changes identity; same asset with two recipe owners retains separate descriptors. Document whether owner is included in requested-input identity and why. It must never stand in for physical bytes.
- [ ] Run targeted Release integration test only, with sole compiler lane, BetterSSD Cargo target, durable Python child returncode, raw log and before/after source freeze. If a test fails, diagnose before modifying assertions or tolerances.
- [ ] Run engine-api adjacent/full tests and crate formatting/strict Clippy only after focused success. No app/GPU/real RAW or workspace-wide build.
- [ ] Independent reviewer audits exact public boundary, serialization roundtrip, rejected inputs, identity claims and gate evidence. Root integrates exact reviewed source/evidence only after acceptance; preserve all draft/failure history.

## Concrete test examples

```rust
let recipe = Recipe::new(ImageId(1));
let bytes = recipe.to_json().unwrap();
let valid = PinnedRawDescriptor::new(input_for(bytes.clone())).unwrap();
let reopened = PinnedRawDescriptor::from_json(&valid.to_json().unwrap()).unwrap();
assert_eq!(reopened.recipe_json(), bytes.as_slice());
assert_eq!(reopened.input_identity(), valid.input_identity());
```

`input_for` is test-only construction of `PinnedRawInput`: asset `Digest::derive("tessera pinned RAW asset v1", b"fixture bytes")`, byte length 13, owner ImageId(1), LibRawCfaV1 route, `arw` suffix, and a non-authoritative optional locator string. This is a descriptor fixture, not a real RAW file.

```rust
let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
value.as_object_mut().unwrap().remove("schema_version");
assert!(PinnedRawDescriptor::new(input_for(serde_json::to_vec(&value).unwrap())).is_err());
value["schema_version"] = 2.into();
assert!(PinnedRawDescriptor::new(input_for(serde_json::to_vec(&value).unwrap())).is_err());
```

The new module does not imply a decoder or source-backed compositor node is shipped. Future consumers must verify captured bytes and separately enforce decoder/environment/render admission; neither a successful descriptor parse nor input identity authorizes pixel publication.
