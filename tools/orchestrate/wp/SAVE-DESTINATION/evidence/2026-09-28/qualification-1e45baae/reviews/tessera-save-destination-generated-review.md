# Save destination regenerated bindings — independent source review

Verdict: no actionable finding. The generation09 bindings are source-approved for the combined native source at `5f34221d4dcd64a93a6de596e90c487d3ea31ffc`. This review does not claim Swift compilation or runtime success; the separate compiler-lane owner supplies those gates.

## Scope and exact delta

Checkout: `/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera`.

Compared working generated sources to HEAD and read native `crates/tessera-ffi/src/document.rs`. The complete diff adds 164 Swift lines and 11 header lines, with no deletions or unrelated edits. It adds `saveAsChecked(path:intent:) throws -> DocumentSaveAsResult`, its protocol requirement, two typed enums and RustBuffer converters, the native C function/checksum declarations, and one initialization checksum check.

Native enum declaration order agrees with the generated wire tags: `CreateIfAbsent`/`createIfAbsent` = 1, `ReplaceConfirmed`/`replaceConfirmed` = 2; `Saved`/`saved` = 1, `DestinationExists`/`destinationExists` = 2. Both converters reject invalid tags. Generated enums retain Equatable/Hashable and Swift 6 Sendable. The generated method clones the DocumentSession handle, lowers String and intent, uses BridgeError-aware calling, and lifts the typed result, matching native `Result<DocumentSaveAsResult>`. Native intent maps to CreateIfAbsent versus Replace commit modes. Legacy `saveAs` remains unchanged with checksum 45094.

The new checksum is **46144**. Read-only disassembly of the generation dylib's `_uniffi_tessera_ffi_checksum_method_documentsession_save_as_checked` returns `mov w0, #0xb440; ret`, confirming 46144 without loading or executing native code.

## Smart Preview preservation

All 434 pre-existing Swift checksum checks are identical; the new checked Save As checksum is the sole addition (435 total). The full diff has no changes to existing Smart Preview types, APIs, converters, or header declarations. All Swift lines containing SmartPreview/smartPreview/smart_preview are also byte-identical. Preserved engine checksums:

- `uniffi_tessera_ffi_checksum_method_engine_open_smart_preview_develop_session`: 44215
- `uniffi_tessera_ffi_checksum_method_engine_open_smart_preview_library_session`: 4323
- `uniffi_tessera_ffi_checksum_method_engine_build_smart_preview`: 4018
- `uniffi_tessera_ffi_checksum_method_engine_discard_smart_preview`: 43019
- `uniffi_tessera_ffi_checksum_method_engine_smart_preview_info`: 49720
- `uniffi_tessera_ffi_checksum_method_engine_synchronize_smart_preview`: 5803
- `uniffi_tessera_ffi_checksum_method_engine_smart_preview_thumbnail`: 32945

## Generation09 provenance and independent hashes

Evidence: `/Volumes/betterSSD/tessera-validation/save-destination-current-main/09-regenerate-01`.

Recorded command is `bash apps/mac/build-ffi.sh`, with the fixed BetterSSD Cargo target, jobs 2, Rayon threads 2, and `MACOSX_DEPLOYMENT_TARGET=15.0`; inherited TESSERA variables were removed. Generation exited 0. Before/after HEAD is unchanged; exactly the two generated tracked files changed. Log records bindgen using the release dylib and confirms the arm64 archive. Source hashes independently recomputed during this review all match generation09 after.json, with no current source mismatches. Both installed generated files are byte-identical to their build/ffi counterparts. All four current ignored FFI artifacts match after.json.

SHA-256:

- `apps/mac/build/ffi/CTesseraFFI.h`: `66598c0c7f27f28fd8605f1b41ad7521b20ec054c902d3d57cf15f5fd621fd45`
- `apps/mac/build/ffi/CTesseraFFI.modulemap`: `efda206de8cf8eb6c092c29fd32f286b9a46d9d7c4c150e6e9b66941dc43d6bd`
- `apps/mac/build/ffi/TesseraFFI.swift`: `daf7b29d4cb29838fc18eb37a5f28b1bfd8eff7c892babe0d0a7608f4e110898`
- `apps/mac/build/ffi/libtessera_ffi.a`: `5b7e5eba7909c4641b4108a3ed3c92f7cd6d204f1a792d0e3660a12c7d162594`
- Native `crates/tessera-ffi/src/document.rs`: `1a6277ad23db2234cf832a6bf9051f37af2feea9ffcc245c8009b17044e842fc`
- Release generation dylib: `7332276e93ac41e4502a9463f4285d409d07e4006009dc93cf13dd20cd2dc084`

The source fixture remains SHA-256 `bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8`; independently rehashed without modifying it. Archive provenance here is the recorded successful generation command plus independently matched archive hash, not a new link/runtime test.

Machine-readable detail: `/tmp/tessera-save-destination-generated-review.json`.

No builds, apps, GPU workloads, source edits, or main merges were performed by this reviewer. Only review artifacts under /tmp were written. One initial read-only objdump invocation lacked its disassemble action and printed usage; the corrected invocation succeeded as recorded above.
