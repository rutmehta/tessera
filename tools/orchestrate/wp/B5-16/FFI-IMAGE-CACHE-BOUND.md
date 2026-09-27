# FFI retained image cache source candidate

Request04e1a883 validated for exact B target, expiry and resource hold, then
accepted before work. Contract read at mainb1b6af5a. Separate slice after CPU
frame candidatef518c03; no other product files or A-owned Swift seams changed.

## Policy

`Inner.imgs` now holds an ImgCache with named defaults of 512 MiB and four entries.
The charged payload is each retained Vec<f32>'s **capacity** times size_of::<f32>,
using checked multiplication. The sum admission check is also checked. Entry/key,
Arc and Vec metadata are not included. Every insertion removes an existing same
key first; an oversized replacement removes that old key but does not evict
unrelated entries. Accepted replacements become newest. Eviction is oldest-first,
as before; lookups do not refresh age. Entries too large for the allowance, or
with overflowing charge, are not retained. Zero budget disables all retention,
including empty payloads. Current caller-owned images and pixels are unchanged.

Prefix storage preflights policy before calling Img::clone. A private copy-closure
seam permits a deterministic counter in tests; the production call supplies
Img::clone. No queue lock spans the deep copy. Insertion rechecks the copied Vec's
capacity and current policy before retaining it. Conservative preflight uses
original capacity, so spare oversized capacity can cause retention to be skipped
even if a hypothetical clone could shrink. Source images already needed for the
current operation only incur the existing cheap Arc clone before insertion.

This is a bound on payload referenced by this cache, per FilterState. Evicted
images can remain alive through active caller Arcs, and concurrent copies,
evaluation temporaries, bakes, detail/mask surfaces, compositor caches and GPU
allocations remain outside it. This is not process-wide memory admission or a
resolution of B's swap incident.

## Seven UNRUN deterministic test cases

All new `image_cache_tests` source is UNRUN and uncompiled on B:

1. Budget64bytes: two four-pixel entries evict oldest; retained bytes stay64 and
   an existing caller reference keeps its original pixels.
2. Replace one key with smaller payload: charge updates once. Five-pixel/80byte
   replacement is skipped and old key removed without changing new pixels.
3. Vec spare capacity, rather than length, determines admission; arithmetic
   overflow rejects without a huge allocation.
4. Four-entry limit with spare byte capacity, and zero allowance retaining none.
5. Oversized prefix at budgets0/64 invokes the deep-copy counter zero times;
   current pixels remain intact.
6. Admitted copy can acquire the queue mutex (proving no lock across copy);
   a test-only policy change during copying is rechecked at insertion.
7. Tiny2x2 source cache hit shares its Arc; two Gaussian stages produce identical
   pixels with cache disabled, on first cached evaluation, and on prefix reuse.
   Retained cache stays<=64bytes and contains the prefix key.

Local checks: existing rustfmt formatted the source; git diff --check passed.
Neither is compilation or test evidence. A validation: focused lib filter
`image_cache_tests`, two build/Rayon workers and outer watchdog, then existing
filter/cache/cancellation regressions and strict lint as appropriate. The parity
case uses a tiny CPU compositor/source render; it does not open windows or create
GPU work. Preserve initial failures. No B build/test/app/benchmark or heartbeat
restart; A owns all main merges.
