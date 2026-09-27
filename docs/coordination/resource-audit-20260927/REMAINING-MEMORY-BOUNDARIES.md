# Source-only remaining resource boundaries (A, 2026-09-27)

Scope: `fb4d7df8` source, no build/profiling/GPU run. This does not identify the cause of B's 43 GB swap event.

1. **FFI filter image cache is count-limited, not byte-limited.** `crates/tessera-ffi/src/document/filters.rs:686-690` defines `Img` as `Vec<f32>` straight RGBA, 16 nominal bytes/pixel. `Inner.imgs` at lines 1299-1300 holds `Arc<Img>` source/prefix images. `store_img` at 1396-1403 keeps four entries regardless of each `Vec`'s byte capacity. A 20 MP image is ~320 MB of nominal pixels; four distinct entries could retain ~1.28 GB in one filter session, excluding clones and other caches. `source` at 1437-1442 and prefix storage at 1590-1593 use this cache. This is separate from the compositor's per-frame `FilterPassLimits` (1 GiB/256 entries at `render/smart_filters.rs:315-344`) because FFI's `Img` cache is outside that pass.

2. **Budgets are per-instance and do not cap transient work.** `FilterState` creates a 256 MiB CPU compositor at `filters.rs:1320-1339`, but its `imgs`, `mask_thumbs`, detail surface, bakes, and presented document are separate fields. `render_region` allocates an output `Vec<f32>` and stores tile results before assembly (`filters.rs:803-833`); `filtered` clones a cached/source `Img` and can clone a prefix again (`filters.rs:1577-1593`); native stack paths allocate whole outputs and crops (`filters.rs:1523-1547`). The merged pass admission explicitly excludes evaluator temporaries, tile caches and GPU memory (`render/smart_filters.rs:315-316`). GPU `ResidentRenderer` defaults to a 2 GiB *page-pool preference* (`resident/mod.rs:529-536`); `ensure` explicitly grows past it if necessary (`1018-1053`). Each nested child creates its own renderer with the same budget (`resident/filters.rs:297-307`); stage buffers are allocated separately (`114-129`), while retained stack cache entries are budgeted (`52-76`). None of these figures is a process-wide CPU/GPU working-memory cap.

**Smallest next repair:** add a per-`FilterState` byte allowance to `Inner.imgs` alongside its four-entry cap, charging `Img.px.capacity() * size_of::<f32>()` with checked arithmetic. Evict oldest entries until both bounds hold; if a single image exceeds the allowance, skip *retention* while allowing the current evaluation to finish. Keep `cached_img`/`store_img` public behavior and source/prefix keys unchanged. A conservative initial allowance such as 512 MiB would retain one nominal 20 MP f32 RGBA image, but the value is a product choice; expose a tiny override in module tests. Do not describe this as admission or total-memory protection.

**Measurable tiny acceptance:** with 64-byte test allowance, insert 4-pixel images to prove the second distinct key evicts the first and current retained bytes stay <=64; replace a key and prove accounting updates once; try a 5-pixel (80-byte) image and prove it is not retained but its returned result/pixels remain correct; verify cache hit/miss behavior for source and prefix paths at tiny size. Record retained bytes/entry count and, if instrumented, cache evaluations separately. A later independent slice should inspect `mask_thumbs` and transient allocation peaks; this proposal does not cover them.

## Coordinator scope for the next bounded slice

Use a named 512 MiB per-FilterState retained-image allowance alongside the existing
four-entry limit. This permits one nominal20MP float image but bounds four-image
retention. This is a cache policy, not a hard process cap. Charge checked Vec
capacity bytes, define duplicate-key replacement, and retain current result pixels
when an entry is too large. Critically preflight prefix retention before
`Arc::new(cur.clone())`: rejecting an already-cloned oversized prefix would still
create avoidable large transient copies. Never hold the queue lock across a deep
image copy; verify/recheck admission before inserting if needed. Test a tiny
allowance plus zero allowance, oversize and replacement, and prove oversized
prefixes skip the copy (test-only counter/hook or ownership structure) without
changing rendered pixels. Source work only on B; A owns bounded execution.
