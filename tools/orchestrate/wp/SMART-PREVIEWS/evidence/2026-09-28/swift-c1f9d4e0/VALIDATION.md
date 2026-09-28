# Swift offline thumbnail integration qualification

Frozen feature candidate `c1f9d4e00c14ce4d7614706fed63963e9b6afe4a` (c1f9d4e0), clean checkout. Native component is separate4cf91289; generated output is separatec1f9d4e0. Root independently reviewed and merged into main89b78881 after final08; this agent performed no main merge. No GPU proposal applied or source preference default changed.

## Integrated sources and generation

Authorized B commits were cherry-picked in order with no conflicts:3262cc6a→e970c7d1,9d35c5d6→32447671,fe5d7911→766811f3,51e0cfb6→3d9856b7,c1644605→cceb20ea,32d19363→4f6425c2,262b6c1f→a9c42139,d40354e4→b5e3cf58. Root/source reviewer approved their routing,invalidation and boundedretry behavior. No Swift product or test repairs were needed during this integration.

01 build-ffi.sh exit0 regenerated matching macOS15 arm64 archive+bindings using explicit CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated and CARGO_BUILD_JOBS=2; MACOSX_DEPLOYMENT_TARGET=15.0 recorded. Generated tracked delta is exactly37 additions across Swift/header for smartPreviewThumbnail(imageId:maxPx:), C entrypoint and checksum32945; existing APIs/checksums unchanged. Root approved generated delta. Manifest differences were exactly2trackedbindings,2buildcopies andstaticarchive; modulemap unchanged. See generation-changes.json and native-output-hashes.json.

## Passing runtime gates

-02 actual Sony offline cached-thumbnail contract:1passed,4.521stest runtime. This is RED→GREEN relative preserved swift-full-final/14 actual thumbnail timeout60s. Production EngineLibrary cached factory, newnativeAPI and ThumbnailLoader were exercised with a disposable RAW COPY; no injected success endpoint.
-05 actual Sony offline Library/Develop workflow:1passed,4.517s. Compact1640x1092 build, declaredreadonlylibrary, exposure/WB rendered intoIOSurfaces, save/close/reopen offline, reconnectsync and Original route edit persistence.
-06 focused Swift:67passed0failed covering SmartPreviewUI, offline routing, proxythumbnail/retry/invalidation, queue, flightdrain, events and viewport ownership.
-07 strict Release Tessera product:exit0 with explicit -strict-concurrency=complete and -warnings-as-errors. Productartifact captured in strict-product-artifact.json.
-08 final full Release Swift:exit0;689XCTest cases with1skip and0failures (688passing), plus5SwiftTesting tests passed. Realfixture env enabled: both actualnativeworkflow tests passed again (thumbnail4.766s;edit/save/reopen4.328s). Existing opt-in generated20k-file EngineLibrary measurement was the only skipped XCTest. Full runtime198.364s; command elapsed353.2s includingrebuild. AgentReviewLayout regression tests also passed.

No Swift behavior/test failures occurred in this integration. Full/testbuild logs retain four pre-existing test-target warnings (three weak-var mutability, one non-Sendable testselfcapture); strict productionbuild was clean. Native historical mechanical lint/fmt failures are preserved underthumbnail-native. Previous Swift mixed-agent/full-suite failures andthumbnailRED remain preserved under swift-full-final; none reclassifiedaspassing.

## Exact evidence and scope

02/05/06/07/08 each have directexit, command/env, logs and equalbefore/after source+archive manifests. 05–08 exactly match FINAL-SOURCES.json, includingHEAD;02 has identicalfilemap beforegeneratedcheckpointcommit. FINAL-SOURCES contains7018Git files plus4ignoredgenerated/archive inputs (7022total), SHA256 cadc327b020d0d5f1facc5b5911f53e89cd1e3d0851ff329746fb08b91671eda. Root independently verified these inputs. Final test/product hashes are in final-swift-artifacts.json. No compiler overlap; source remained frozen throughfull08 and evidencefinalization.

03 handwritten diff check excludinggenerated2files passed.04 plain diffcheck intentionally records exit2 for generator-emitted whitespace-only lines C3560/Swift7575/8428. Root approved preservingcoherentgeneratedoutput; no handediting and no claimofglobaldiffchecksuccess.

Sourcefixture remained byte-identical before/after: 16646144bytes, SHA256 bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8. fixture-before.json==fixture-after.json. Tests separately protecthiddenphoto-copy/sidecar hashes duringofflineedits and assert no originalfolder recreation; reconnectwrites only the disposablecopy's sidecars. No privatephotoasset published or committed.

FunctionalofflineLibrary/Develop/thumbnail bridge acceptance is supported by thesegates. This is not manualGUI acceptance, a GPUqualification, or an interactive-speed claim. Original remainsdefault; CPUproxy editperformance limitation remains tracked. Nativeboundedqueue overload is honestly reported; Swift retries at250/500/1000ms then terminateswithoutpixels onexhaustion (placeholderuntilrerequest, no infinitepending or Originalfallback). No new per-image failureUI claim.

Compiler lane RELEASED after final08/evidence; noactivebuild/test process remains. NextGPUwork requires its ownexplicitqualification.
