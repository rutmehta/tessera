# Smart Preview decision-reuse plan — independent source review

Verdict: approved as a bounded implementation plan, with the capability-check clarification below made explicit in implementation/tests. No architectural correctness blocker found. This is source review only: no cache exists, no implementation/runtime acceptance is claimed, and no repository files were changed.

Reviewed main HEAD: `cc9fe4a7c4aab11868b29b6d00aa88cdd3c0592f`. Plan SHA256: `8e71e9a92a09e73e23ab68277b908bdb12a1ca3cbf9b82a80dcc7e03c9f81cbd`. Read the plan against Engine selection, Develop admission/resource construction, validated proxy load/journal handling, RendererConfig/PipelineGraph, GPU device health, resident capability/dispatch, and scalar settings validation.

## Required clarification before integration

**Use normalized calibration settings and the candidate GPU renderer for eligibility, including when reusing a measured CPU decision.** `backend.rs::select_proxy` clears only `output.hdr` and `hdr_headroom_stops` on a temporary clone before SDR calibration. `Renderer::can_render_resident` delegates to `camera_linear_resident_supported`, which calls scalar settings validation; raw HDR settings fail that validator. Also CPU operators cannot advertise GPU residency. A capability check on the full HDR recipe, or on the selected CPU backend, would permanently bypass legitimate EDR or measured-CPU hits. Keep full untouched settings in the key; use the exact existing SDR clone solely for capability/calibration; evaluate the available GPU candidate's capability independently of the cached winner. Check failure must bypass reuse without introducing a new public-open error. Add explicit tests for unchanged HDR hits and measured-CPU hits with a healthy, capable GPU candidate. This is a potential false-miss/integration problem, not evidence that existing rendering is incorrect.

## Correctness and dependency findings

- Full container digest plus original identity, journal incarnation/generation/document digest and typed settings is sufficient for the proposed immutable proxy/calibration inputs. Render ImageId alone is correctly excluded. Captured metadata, calibration and resolved lens are covered by container identity. Obtain incarnation from the existing opened journal handle, not an additional independently raced snapshot.
- Existing open ordering is important: stable-ID admission, decode/source validation, pending-intent reconciliation, document and online sidecar checks, then resource creation. Reconciliation checks the journal and may remove acknowledged intent; it does not rewrite the returned recipe snapshot. Carry identity alongside that same result, and never let a cache hit short-circuit any of these checks or later resource creation errors.
- External assets need an explicit deny-by-default eligibility predicate, as the plan requires. Current scalar validation rejects nondefault camera-profile/LUT controls; proxy recipe validation itself checks Native2 and captured prefix, so it is not a substitute for the renderer's complete support check. Initially require no local adjustments/retouch and no external profile/LUT controls, plus successful normalized resident capability. Future permissive validators must not accidentally expand cache eligibility. Tests should assert bypass, not resolve or hash a named external file opportunistically.
- Effective config and complete ordered graph descriptors are covered. Device generation plus immutable Engine OnceLock device and adapter/capabilities covers this Engine's device lifetime; any future reset must change generation. Do not substitute configuration defaults for the values used to construct the measured operators.
- Geometry refusal is shared by capability and dispatch before resident execution; retain it. A cached Metal selection is not a promise that later mapped edits execute on GPU. Explicit override branches must preserve their current early-return behavior, including CPU avoiding device initialization.

## Bounds and performance limits

Fixed 16 copied entries, an asserted size bound, brief mutex scope and fresh operators preserve ownership boundaries. No retained image/backend/surface or pending-work list is needed. Error outcomes must stay distinguishable from successfully measured CPU winners. The existing measured winner retains warmed caches on a miss; preserve that, rather than reconstructing it unnecessarily.

Full document/generation/settings keys intentionally miss after harmless history or presentation changes. Those misses affect benefit, not fidelity. Thirty-second non-sliding expiry reduces stale performance choices but does not ensure the thermally fastest backend. Concurrent identical misses may duplicate calibration; this is explicitly bounded per request rather than a new global concurrency guarantee. Keep completion time as the TTL origin, not delayed insertion time.

## Required acceptance still outstanding

Pure key/bounds/TTL/LRU and bypass tests; real prepopulated-cache corruption/source/sidecar refusal tests; healthy-device CPU/HDR hit tests above; mapped/reset route assertions; heavy-owner release checks; then immutable native and Swift gates. The preregistered combined open-to-first-FINAL measurement is appropriate because skipping calibration also removes warmup. Exact-key hits must actually have zero calibration frames. Conservative false misses or cold fresh operators that defeat the predeclared benefit are reasons to reject the optimization, not weaken identity or numerical bounds.

The baseline harness `c77e0543` remains awaiting its separately assigned runtime qualification. No speed result is established by this review.
