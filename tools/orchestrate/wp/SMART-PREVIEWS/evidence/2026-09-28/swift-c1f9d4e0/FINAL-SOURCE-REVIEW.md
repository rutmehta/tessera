# Final Smart Preview integration source review

Scope: immutable feature c1f9d4e00c14ce4d7614706fed63963e9b6afe4a against main 3c45371f. Read-only review; no source mutations, builds or tests. Full Swift08 runtime gate is owned by the compiler agent and was still running when review was requested.

Disposition: no blocking product defect or unexpected production drift identified in the remaining integration delta. Source-approved subject to the separately owned runtime gates and preserving main-only documentation/evidence.

## Candidate reconciliation

- All Swift app/Core production files at c1f9d4e0 match reviewed B d40354e4 except the already-reviewed Document warning/keyboard/wrapper corrections. Those Document files have no remaining difference against main. No additional Smart Preview UI implementation change is hidden in this final feature head.
- Native delta is exactly commit 4cf91289 (five files): bounded local thumbnail scheduler, readonly local journal access, additive guarded preview-cache read/write helpers and tests. Compared against /tmp/tessera-smart-preview-thumbnail-src, previews/lib.rs and smart_preview_store.rs are byte-identical. Remaining differences are only the already-approved two equivalent let-chain collapses, module declaration ordering, and unused test import removal.
- Thumbnail Swift production commit 766811f3 is the reviewed source-role/invalidation candidate. Retry a9c42139 is the reviewed three-backoff proxy-only candidate; cceb20ea/4f6425c2 carry its deterministic cancellation/exhaustion tests. Test/UI content matches B d40354e4 apart from the separate native workflow and already-reviewed Document tests.
- Native workflow 4b5f9c70 retains the reviewed actual Original edit/commit/close setup, offline edit/reopen/sync path, bounded thumbnail polling, disposable copies, explicit Sony dimensions and corrected relative-name preservation-map helper.
- Generated changes come from 2fb3b1f0 and c1f9d4e0. The final commit adds only smartPreviewThumbnail to the C declarations/checksum, Swift protocol/wrapper and initialization checksum. The overall main delta also exposes previously integrated native Smart Preview build/info/discard/sync/develop/library APIs and their record/enum converters. Existing depthHistogram/openDevelopSession checksum changes accompany their corrected native documentation metadata. No hand-written fallback or stub appeared. Deployment still requires the corresponding regenerated native archive; source review alone does not establish binary checksum agreement.
- Earlier UI/routing work in the remaining main delta is already covered by the reviewed controller/status/original-default and canonical offline Library candidates: 567f5e92 integration plus b6cea8f2 mechanical import/test correction; upstream canonical authority fix 3029127b; same-photo refresh dd84ed57 and autosave presentation 8d35e9f6. Canonical successful-open persistence and read-only cached session routing remain intact.
- GPU candidates and qualification hooks remain absent from the product delta. No GPU performance claim or enabled proxy GPU route is implied.

## Integration caveat

Do not replace main's whole tree with the feature tree. The two-tip comparison includes deletion of main-only coordination/evidence/proposal records and regression of the plan's completed checkboxes to older unchecked content. These are branch-history differences, not intended feature deletions. Preserve main's newer docs/status/evidence while integrating the product paths below; reconcile the two B handoff documents separately if desired. Native ownership/export/agent fixes and Document corrections already present on main require no duplicate product change.

## Exact remaining product/test/generated paths

- apps/mac/Sources/CTesseraFFI/CTesseraFFI.h (feature blob 4f29f98dc80d2a48195e79773366be585bcb3383)
- apps/mac/Sources/Tessera/Agent/AgentController.swift (feature blob 77295f153d2e15cb50d9fb56f4b88760d91551c8)
- apps/mac/Sources/Tessera/App/AppCommands.swift (feature blob a5a5b13d2c9404b67ab9452a467e0f169688a9e6)
- apps/mac/Sources/Tessera/App/AppModel.swift (feature blob 0a1b2849684f8189861259c964805ec21d745079)
- apps/mac/Sources/Tessera/App/TesseraApp.swift (feature blob d4f03f8a9725f6b0d43b417eecc98c5f5964ce35)
- apps/mac/Sources/Tessera/Compare/CompareView.swift (feature blob 5198020c3f1cc798b1e0528c212856323aae6bd7)
- apps/mac/Sources/Tessera/Cull/AssistController.swift (feature blob 988a810fbc00a19b0d226d6aa0dec37d841f9c6d)
- apps/mac/Sources/Tessera/Grid/ThumbnailBrowser.swift (feature blob 188c8922d5cd0730ef7fb0088586e5dea610ec72)
- apps/mac/Sources/Tessera/Grid/ThumbnailCell.swift (feature blob 59b563c0a08fab38c55abab1384240e0144f047d)
- apps/mac/Sources/Tessera/Inspector/InspectorView.swift (feature blob e5b8da27b5ce2fb8383539a964385a11cd82efab)
- apps/mac/Sources/Tessera/Library/LibraryModel.swift (feature blob 16dc58600508916a1ff9b256cc314d37925e775b)
- apps/mac/Sources/Tessera/Library/SmartPreviewMenu.swift (feature blob 200544c748eb6f9e5afdfe5cde891f1b47eac114)
- apps/mac/Sources/Tessera/Library/UnderstandingController.swift (feature blob a09ad823b659d5bef31939d7ff0d2a5b53307692)
- apps/mac/Sources/Tessera/Loupe/LoupeView.swift (feature blob 512ed5d3c723433ac228d2ff990ee2025d942eaa)
- apps/mac/Sources/Tessera/Shell/ContentView.swift (feature blob c01b9ce6f283024c2a403988275cc888c2dc08bd)
- apps/mac/Sources/Tessera/Shell/WorkspaceHeader.swift (feature blob dd9db4d6b0b485136f3e45ee35c9a5b8b6ef3413)
- apps/mac/Sources/TesseraCore/Assist/CullController+Assist.swift (feature blob 288a22f2d1d6c38c31faa1007fe2df7a7ca4124b)
- apps/mac/Sources/TesseraCore/Assist/People.swift (feature blob be81aea5f99bb2bb97d7c2b335d39db68c2c593c)
- apps/mac/Sources/TesseraCore/CullController.swift (feature blob 1e0625eb1afbc9942bc162c2fe9ed4145740ee76)
- apps/mac/Sources/TesseraCore/DevelopController.swift (feature blob 4b63b1fd5b0149993e8e707bdc21abc5a767ea33)
- apps/mac/Sources/TesseraCore/EngineLibrary.swift (feature blob cf2b473335f622bf9d91da81e0096905d33fe91c)
- apps/mac/Sources/TesseraCore/LibraryOpenRouter.swift (feature blob 5273827c7b8048a30e60350ab7d8f5528a12c975)
- apps/mac/Sources/TesseraCore/SmartPreviewController.swift (feature blob a87bd687e4c39f567baf4c79754428f2b9d9c03d)
- apps/mac/Sources/TesseraCore/SmartPreviewNative.swift (feature blob dd18896d6710448bf440b66e456f77e27f0211e9)
- apps/mac/Sources/TesseraCore/ThumbnailLoader.swift (feature blob 0f034519eb7878c6f8b8783bf04033a25b14306d)
- apps/mac/Sources/TesseraFFI/TesseraFFI.swift (feature blob 2997dbb264f4e85b4a9b742f3e4b599cd8dbc811)
- apps/mac/Tests/TesseraCoreTests/OfflineLibraryRoutingTests.swift (feature blob 02df0bb91add95f8ff91eaa90709dd6a4c51f3cf)
- apps/mac/Tests/TesseraCoreTests/SmartPreviewNativeWorkflowTests.swift (feature blob 0845368d26a46d3758943042df569589748ba813)
- apps/mac/Tests/TesseraCoreTests/SmartPreviewThumbnailTests.swift (feature blob 18a057abb06ed8b55977cdab470b71bb6149f4e0)
- apps/mac/Tests/TesseraCoreTests/SmartPreviewUITests.swift (feature blob bb3a5f3005cf135e9b76ffb2812e153338919df2)
- apps/mac/Tests/TesseraCoreTests/WorkspaceReadyPhotoTests.swift (feature blob 22571e11f7d4f5f9476af6d529247353aedb423f)
- crates/previews/src/lib.rs (feature blob 2bf036b038141f3dc27d0b5be50b9720697cae85)
- crates/tessera-ffi/src/lib.rs (feature blob aecad76094630977873493c660335aee92b03057)
- crates/tessera-ffi/src/smart_preview_store.rs (feature blob a944263e729c59794b9fe8c7355d4ee20484cd4d)
- crates/tessera-ffi/src/smart_preview_thumbnail.rs (feature blob 68c1dfcda9fb030a1bb394752f684ab4775c7802)
- crates/tessera-ffi/src/smart_preview_thumbnail/tests.rs (feature blob 269f60248df96432f46990711fce86bf343f7f80)
