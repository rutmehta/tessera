# Inspector 858147a3 — independent automated provenance review

**Approved automated evidence at `858147a3183c98b9e15118495a75da3bc2ddd0d0`.** No GUI acceptance or main merge is implied. Read-only source/blob/artifact/log inspection; no compiler, app or GPU workload launched.

Independently streamed the immutable Git archive and checked all8,383 captured source-file SHA256 values. Prepared baseline and every before/after source map are identical to Git. Focused, layout, full and strict all have direct exit0, identical source/HEAD/ignored-FFI/fixture freezes. Strict command includes complete concurrency checking and warnings-as-errors.

- Focused models/analysis:67 XCTest passes,0failures; all15 mandatory named tests actually passed.
- Layout:4passes,0failures; all4 mandatory named layout cases actually passed.
- Full:720 XCTest cases,1skip,0failures;5 Swift Testing passes. Independently checked105 unique mandatory named pass lines, including focused/layout/adjacent contracts and both actual native Smart Preview offline workflows. The sole skip is LibraryTests.testTwentyThousandEngineLibraryMeasurement, an opt-in generated20k-file fixture. No omitted mandatory test is counted as passing.
- Strict product build:exit0. Its executable differs from the ordinary test-build product as expected; identities below distinguish them.

Ignored FFI files remain unchanged through every gate and match current disk: header66598c0c7f27f28fd8605f1b41ad7521b20ec054c902d3d57cf15f5fd621fd45; modulemapefda206de8cf8eb6c092c29fd32f286b9a46d9d7c4c150e6e9b66941dc43d6bd; Swift bindingsdaf7b29d4cb29838fc18eb37a5f28b1bfd8eff7c892babe0d0a7608f4e110898; archive5b7e5eba7909c4641b4108a3ed3c92f7cd6d204f1a792d0e3660a12c7d162594. This verifies inherited artifact identity, not a newly executed native rebuild.

Read-only Sony fixture bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8 agrees with every freeze and current file. Full log discloses the existing optional Lens Blur missing-model export warning; do not claim all optional models were exercised.

Executable SHA256 after strict (and independently matched to current disk):
`f0f676ce47483531916923684b7c2ab817596d29b9c15c91c7e51363ec9d1171`
Path: `/Volumes/betterSSD/tessera-validation/b5-16-history-cd0b850d/swift-build/release/Tessera`.

Ordinary focused/layout/full product: `bcb1bdab3d7107e4ced3fd3eb122f1377da6f7a8d81f353e1d32df368e166640`.
Test bundle executable: `5591754bbb2f4ab8c52142a899695705d8f6a3e83cbf0eab86a811d4657fd52c`, unchanged through final strict. Prepared inherited7732 binaries have separate hashes and are not mistaken for final outputs; scratch-directory name cd0b850d is reused storage, not the tested source revision.

Detailed independently recomputed gate inventories, source count and artifact/output identities are in `/tmp/tessera-inspector-858147a3-final-review.json`. The automated evidence is coherent; actual GUI and user-visible behavior remain separate acceptance work under coordinator ownership.
