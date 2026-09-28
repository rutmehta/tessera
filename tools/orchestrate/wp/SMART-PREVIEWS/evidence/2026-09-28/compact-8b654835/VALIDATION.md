# Compact Smart Preview tier validation

Commit8b654835 in feature checkout, clean worktree. All commands complete; sole compiler lane released. No main merge by implementer.

Explicit Compact2048 tier uses integer camera-linear area reduction; Detail2560 remains generate() default. Writer version2 records tier/generator2. Version1 generator1/no-tier remains readable as Detail2560; conflicting/missing/unknown tier/version/generator combinations fail closed. Original metadata/calibration/prefix/resolved lens retained. No FFI/GPU source change, no DNG or interactive-speed claim.

## Gates at frozen committed inputs

01 focused codec:12 passed,2 opt-in ignored. 02 image-core Smart Preview:7 passed including persisted Compact scale3 L0/L1/L2 route. 03 preliminary strict passed.
04 full Release pipeline-cpu + image-core:257 passed,0 failed,5 ignored across51 result blocks;122.2s. 05 real Sony Compact:1 passed. 06 actual preserved v1 asset:1 passed. 07 final strict alltargets Release passed. 08 final scoped format passed. No failures in this qualification wave. Every numbered command includes argv/environment/direct exit and equal source hashes before/after. Explicit shared CARGO_TARGET_DIR used throughout.

## Sony Compact measurement (single bounded run, scalarCPU)

Original16,646,144B; sensor4928x3276; activecrop4920x3276. Compact scale3,1640x1092,F16,7,319,246B:56.0304% smaller than original. Existing Detail v1 asset16,632,169B retained separately and untouched.
Raw decode58.207ms, generation523.527ms, encode79.682ms, decode20.888ms. Decoded proxy baseline render132.049ms; WB4200K/tint13/exposure+0.7 edited render134.485ms. Edited output dimensions1640x1092; edit changes pixels. Max stored sample error0.0001220703125, every sample satisfies .0005*abs(original)+3e-8. Max edited linear render difference vs unencoded same Compact proxy0.0005311965942382812; all samples within .003*abs(reference)+.0005. These bounds concern codec fidelity within the same spatial tier, not equality to full-resolution RAW rendering after nonlinear edits. Smaller spatial tier intentionally sacrifices detail.

No matched-output original/default-resident benchmark here. Lower-output scalar timing cannot establish app editing-speed improvement or justify selecting previews automatically. Original remains default; FFI build still chooses Detail until root explicitly changes and separately qualifies it.

## Actual preserved legacy asset

Read existing codec/sony-camera-linear.clp without modification. Confirmed header v1, dimensions2460x1638, decoded as Detail2560, source identity assertions match historical report. WB/exposure edited render hash exactly matches historical measurement. Re-encode/decode v2 retains exact samples and exact edited render. New report in legacy-v1-reopen.json; private asset hashes in fixture-inputs-before/after.json. No private asset copied to Git.

Original ARW, old v1 asset and old measurement SHA256 values unchanged. Original ARW SHA256bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8. All new Compact artifacts/report are in this NEW directory, so original evidence was not overwritten.

Recommendation: Compact demonstrates useful byte savings for this Sony fixture with bounded codec errors and preserved editability/legacy compatibility. Root can separately select Compact for new FFI-built assets after native workflow qualification; keep Original source preference default. Size/fidelity on this fixture does not establish a universal compression ratio or interactive performance.
