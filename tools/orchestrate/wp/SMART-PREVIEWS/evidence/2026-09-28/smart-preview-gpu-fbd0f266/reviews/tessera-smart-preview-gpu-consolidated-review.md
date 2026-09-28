# Consolidated GPU source disposition

Approved as a source-only candidate, conditional on native/Metal/runtime gates. No actionable source defect found. Reviewed consolidated patch against supplied8c5fd747 base, preserving Compact and native build tier behavior. No builds/shared mutations.

Candidate SHA-256: `2c6c31ec86f0c2e878b17a268ff7104a072105ea24098e83a16b54cca2c2e3a6`.

Compared both image-core production renderer files byte-for-byte with the reviewed final coarse sources: identical. Compared all three FFI production files with final coarse FFI sources: identical. Lens-plan source matches previously reviewed GPU source. Pipeline-cpu smart_preview differences from original GPU source are exactly previously accepted Compact tier/constructor changes; resident_tail_plan is inserted after the Compact-aware initializer retaining tier. Image-core tests retain the accepted scale-three test before appended tail-plan test.

New test deltas inspected independently: explicit fixture_tier preserves Detail2560 for prior fixtures; Compact2048 fixture generates, encodes/reopens, and asserts scale3/1640 extent before GPU/scalar SceneLinear/Display/DisplayLinear comparisons across L0/L1/L2/L0 with captured geometry. All GPU contexts go through a helper that fails without a device, asserts Metal and prints adapter identity. Added F32 and L0 surface submission/dispatch assertions prevent CPU fallback from passing those numeric tests. These additions are test hardening, not execution evidence.

No inferred speed claim or GPU default recommendation. Actual GPU comparisons, macOS surface tests, strict checks, public FFI adaptive/viewport workflow, and original/export regression gates remain required. CPU proxy route remains default and GPU opt-in separate from original backend authority. Apply only consolidated patch, not its component patches as well.
