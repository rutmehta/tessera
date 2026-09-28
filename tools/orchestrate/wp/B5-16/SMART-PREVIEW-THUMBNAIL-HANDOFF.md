# Smart Preview thumbnail source handoff

Request9269efdc-91e7-4c49-bb4c-8e2e1db89bae: exact B target validated, unexpired request accepted before changes. New branch `codex/smart-preview-thumbnails` from pinned A integration `b6cea8f25f644754e823e7ffa933cf0c9e840226`. Preserved Document strict186559ce, SmartPreviewUI775f39b0 and SaveAs3ca3e9c9. No main/generated/Rust/Document edits or B builds/tests/apps/benchmarks/heartbeat/writer changes.

Ordered source checkpoints: tests `3262cc6a` then `9d35c5d6`; product `fe5d7911`. Seven new tests UNRUN on B: five SmartPreviewThumbnailTests and two SmartPreviewUITests. Source inspection and git diff --check only. A owns new native API/bindings, compiler, all runtime/GUI/native gates and main. Do not present this source as a working offline app.

## Frozen API and routing

Engine.smartPreviewThumbnail(imageId:maxPx:) -> PreviewResponse, same optional encoded bytes/pending and existing PreviewReady. No handwritten bindings. This branch intentionally awaits A's new generated method. Both current tiers (384/2560) are inside 1...2560. Native owns bounded local proxy/journal validation, exact asset+recipe caching, rendering and stale-result suppression, without original fallback/I/O. No additional API invented.

EngineImageReference gains immutable original/smartPreview thumbnail role, assigned by EngineLibrary: cached read-only Library -> proxy, ordinary online Library -> original. Develop's separate Original default/explicit proxy preference is unchanged. EngineThumbnailAPI is a synchronous injectable worker boundary with separate native functions; ThumbnailLoader calls exactly one by role and never falls back after throw/invalid bytes/missing result. Production requests still run on the existing bounded OperationQueue, including pending-event waiting and cancellation/drain behavior. The synchronous render helper retains its existing caller responsibility.

Swift cache/flight key explicitly contains reference owner identity + source role (tier remains separate). Cached versus online references and reopen ownership cannot coalesce or share pixels. PreviewReady lacks a source field: it remains only a filtered imageID/maxPx wake-up hint. An unrelated role's event can cause another read, but that read invokes the flight's own explicit API and revalidates natively. No event grants readiness/pixels or triggers original fallback. Subscribe-before-render buffering remains unchanged.

## Invalidation and presentation

Existing local Develop save handling already invalidates both loader tiers and notifies cells; it now bumps cached proxy presentation revision for Compare. SmartPreviewController notifies thumbnail invalidation before each build/discard/sync native operation and again after actual return, including failures that may have partially changed native state. AppModel binds it under current controller/image ownership guards; stale controllers cannot invalidate a new Library. No new status hash reads or per-cell polling. Reconnect/reopen retains AppModel.install removeAll, which cancels old subscriptions and retires cached pixels before fresh references install. Loader identity checks/cancellation suppress an old worker's delivery/cache insertion while its underlying native call drains.

Grid proxy refresh clears the displayed bitmap before requesting, so discarded/invalid proxies cannot leave stale thumbnails indefinitely. Loupe first-paint proxy preview invalidation clears/reloads only when no live Develop frame has taken over. Compare now keys presented items by full PhotoItem/reference identity instead of dense numeric IDs and uses Library revision for proxy changes; callback also checks current item. These bounded consumer changes preserve zoom/geometry and live Develop frame ownership. No claim of live GUI verification.

Cached Library badges identify Thumbnail source: Smart Preview; local-save/status-warning separation remains and never authorizes opening. The cached notice describes validated proxy rendering with local saved edits and possible missing thumbnails. Online embedded Library retains its last-synchronized copy. No promise that pending/failed proxy renders are current, and no speed/compression claim.

## Tests and A gates

1. Exact original/proxy API choice, tier maxPx and no original fallback after proxy failure.
2. Populated original cache cannot satisfy same-image proxy request.
3. Buffered ready event arriving inside first pending proxy call resumes same-role render.
4. Invalidating a gated proxy worker rejects its result and waits for drain before replacement.
5. Reconnect/removeAll during gated proxy call isolates new original cache/delivery.
6. Proxy source copy retains local-save warning without last-synchronized claim or routing snapshot.
7. Build/discard/sync invalidate before and after native drain on success and failure.

Thumbnail fixtures use local Engine.open only for reference ownership, injected APIs/encoded 2x2 PNGs, locks/gates for ordering, and timeout bounds solely to fail tests. They do not validate real proxy bytes/cache identities/rendering. A must regenerate binding, strict compile, run new/existing SmartPreview/ThumbnailQueue/FlightDrain/PreviewEvents suites, execute its native expected-RED/offline original-disconnect regression, then actual cached grid/loupe/Compare edit-save/rebuild/discard/reconnect GUI. Check stale displayed pixels, no original metadata access, source ownership, no new full-asset per-cell status polling and bounded queue/memory under realistic data. Cross-process proxy mutations still require explicit refresh/reopen; no new filesystem watcher is claimed.

## Retry follow-up dfb80e63

Exact B request validated/unexpired and accepted before edits. Incremental source from51e0cfb6 on the same thumbnail branch. Tests `c1644605`/`32d19363`; product checkpoint follows in Git history. Separate compiler wrapper10c59dea/handoff2711296e was completed first and published result87e9de81; no compiler/Document edits in this retry diff.

A's native admission cap8 does not follow the Swift loader's subscriber count: canceled Swift pending waits can leave native work running. Previously all thrown thumbnail errors became a terminal empty RenderResult. Now only thrown **smartPreview** request errors set an internal retry flag. The async flight allows three additional calls with cancellable250/500/1000ms waits (four error-producing calls maximum,1.75s aggregate backoff). No string parsing, original fallback, native API changes, global queue or busy spin. Malformed/missing/native admission errors share this same budget; there is no claim every queue-full case will recover inside it.

The existing loader flight retains its slot, subscriber ownership, source key and drain while waiting. The production wait is cancellable Task.sleep; any wait error retires the flight, and an explicit cancellation check after the wait protects against injected waits returning normally after cancellation. Old invalidated results still cannot deliver or repopulate cache. Retries never reset the budget after an event. A successful pending response still uses the existing buffered PreviewReady stream, with no timer/backoff; a terminal nil/nonpending response and ordinary Original errors remain terminal. Budget exhaustion releases subscribers/slot and leaves the existing unavailable placeholder/disclosure, not a fake pending promise. The existing synchronous ThumbnailLoader.render helper does not gain sleeps/retries.

Five new tests **UNRUN on B**: gated transient failure->success while retaining flight slot; persistent failure exactly4calls/3waits and no live flight/cache; invalidation/supersession during a deliberately non-cancellable injected wait rejects old retry and admits replacement after drain; thrown cancellation from wait prevents further native calls; Original errors never retry. Existing pending-event test asserts wait is never used. Prior proxy failure/cache test now injects immediate waits rather than real backoff. Ordering uses continuations/gates, not sleeps. Ten thumbnail test methods plus22SmartPreviewUI now authored; this is no execution claim.

Source inspection and git diff --check only; A must strict compile/run new and prior queue/pending/drain tests, then exercise real scrolling cancellation with native8 admission, visible recovery, persistent corrupt/missing assets, cancellation during backoff, and GUI placeholders. Tune/accept delays only with actual native evidence; sustained pressure can exhaust the bounded retry budget. Native job cancellation, admission and cachewrite fixes remain A-owned. No B workloads/apps/heartbeat/writer changes, no generated/native/Document/AppModel edits for this incremental task. A sole main integrator.
