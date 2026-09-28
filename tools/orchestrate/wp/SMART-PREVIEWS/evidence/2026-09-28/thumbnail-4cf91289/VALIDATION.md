# Native Smart Preview thumbnail qualification

Candidate `4cf91289cf7a3167dd4275b9880611e7bff8e914` (4cf91289), based on4b5f9c70. Five owned files; no Cargo/lock, GPU, Swift, original source preference, codec, or export changes. Separate native commit; no main merge.

Source review approved exact proposal SHA2564ae64fe0258091fcad6edf644143a05aae32aad38905837155b2888f4fc9d81f plus four mechanical qualification repairs (unusedtestimport, two equivalent letchains, moduleordering). Report /tmp/tessera-smart-preview-thumbnail-review.md.

Final gates on identical committed source:
-07 full tessera-ffi Release units:187passed0failed0ignored (includes12newthumbnail tests, existingpreview/journal tests).
-08 full previews Release:28passed0failed3ignored, including boundedhelper and ordinary RAW/JPEG cache revision+linearDNG regressions. Threeignored are pre-existing performance measurements.
-09 strict release Clippy both affectedpackages alltargets -Dwarnings:exit0.
-10 workspaceformat check:exit0.
Earlier01focused12/12passed;02boundedhelper1/1passed. Failures preserved:03strict exit101 onlytwo collapsible_if;05fmt exit1 onlymoduledeclarationordering. No behavioral test failures. Existing LibRaw vendor C++ warnings remain logged; strict Rust checks unrelaxed.

Every command has direct exit, exact argv/environment, log, wholeworktree sourcebefore/after manifests including newlyaddedfiles. Final07–10 allsourceequaltrue and identical to FINAL-SOURCES.json: 7016 files, manifest SHA256 eb60155711fa15abb84f728df7e81adf402348dcda38bc610f46c3f631e3bfc0. Shared explicit CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated; MACOSX_DEPLOYMENT_TARGET=15.0; CARGO_BUILD_JOBS=2. No compiler overlap.

NativeAPI smartPreviewThumbnail(imageId:maxPx:) uses explicit localproxy source, boundedvalidatedjournal/asset and currentrecipe, existing PreviewReady/pending mechanism. Key includes exactasset/recipe plusjournalincarnation/revision; readonlyguardspermitactiveproxyeditor and serialize freshness/publication. Original paths are SQL identity only; generatedstore links rejected; cache readsbounded32MiB and localwrite paths guarded. Interruptedwork retries; terminalerrorsdeliveredonce permit laterretry. Nativequeue8 bound honest error; authorized B retry source will be integrated separately.

Runtime qualification here uses synthetic camera-linear nativefixtures:12cases cover orientation/offline/newEngine, edits, ownership/prefixvalidation, missing/corruptjournal/asset, symlinkread/write avoidance, ABAcompletion, cache repair, cancellation, reentrantcallback, queuebound, activeproxyreads. This is not yet actual Swiftthumbnail or GUI acceptance. Existing realSwiftprimary workflow13passed andthumbnail14RED remain prior evidence; matching archive/bindings and B route integration are next. No new performance/GPU or full-feature success claim.
