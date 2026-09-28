# Explicit FFI Compact generation

Commit8c5fd747, feature checkout clean. Two files only: FFI build selects generate_with_tier(...Compact2048), actual workflow asserts persisted tier/scale/dimensions. Core generate() default remains Detail2560; existing Detail/v1 assets remain readable. No automatic rebuild, discard, migration, source preference, export guard, or GPU change. Original stays preferred.

All commands complete; compiler lane released. All cargo commands explicitly used shared BetterSSD CARGO_TARGET_DIR, source manifests equal before/after each command. Exact argv/environment, direct exits and logs preserved.

01 focused Smart Preview unit tests22 passed. 02 strict preliminary passed. After commit,03 full FFI units172 passed;04 real Sony workflow1 passed;05 alltargets Release strict passed;06 format passed. No failures in this wave.

Real workflow confirms newly persisted Compact2048 asset at1640x1092, integer scale3. Offline clean recipe read and Develop reopen, dirty journal save/reopen, offline library restart, guarded original writes, synchronization and conflict behavior pass. Full-quality JPEG remains4920x3276 and synchronized exposure changes actual pixels. Test4.33s; total66.2s including relink. No interactive-speed claim from these timings.

Original Sony fixture16,646,144B unchanged before/after, SHA256bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8. Disposable copy only; no private assets or generated bindings committed. Core Compact size/fidelity and actual v1-asset parity evidence remains in ../compact-tier at8b654835.
