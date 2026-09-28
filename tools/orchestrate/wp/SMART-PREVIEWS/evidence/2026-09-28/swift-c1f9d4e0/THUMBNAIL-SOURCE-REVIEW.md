# Swift thumbnail source review

Reviewed B production `51e0cfb6` relative pinned `b6cea8f2`, plus added tests, immutable Git sources. No shared edits/builds; current Swift compilation freeze preserved.

No new actionable defect found in this scoped delta. Source-approved conditional on native API integration/Swift runtime gates and separately assigned scrolling-overload handling (dfb80e63).

EngineLibrary produces immutable source-role references: cached Library chooses Smart Preview, online Library Original. Native adapter selects exactly one endpoint, with no fallback. Reference identity already isolates library instances; explicit role in loader key makes routing separation unambiguous. Reopen/install clears existing loader work before replacing the library/controller. Native remains responsible for asset/journal freshness.

AppModel invalidates both loader tiers on saved edits and batch lifecycle changes. Thumbnail invalidation runs before and after native batch operation, including failure, retires flights, increments observed libraryRevision and notifies visible positions. Grid clears stale proxy image before rerequest; Loupe clears/reloads only when showing cached image rather than active engine surface, with current-item guard. Compare tracks full PhotoItem identity plus proxy libraryRevision, cancels prior requests and clears/reloads from current cache. Existing loader generation/cancellation checks prevent an invalidated worker from publishing into replacement state even if item identity is unchanged. Owner/controller checks reject late notifications from an old library.

Presentation distinguishes proxy-rendered thumbnail source from last-synchronized original thumbnails without claiming pending/failed pixels are current. Missing/invalid proxy has no Original fallback. Tests cover exactly-one-endpoint, role/cache isolation, buffered pending events, invalidated-worker suppression, reconnect ownership and thumbnail invalidation/presentation; they are not actual AppKit/Grid/Loupe/Compare runtime coverage and were not run in this review.

Known normal scrolling-overload case is already assigned to B: cancelled Swift flights can leave native pending work, so loader cap4 does not alone guarantee native cap8. This is not redispatched or counted as a new finding. Full UI and real native thumbnail tests remain necessary after that bounded fix.

## Retry delta — d40354e4 / production262b6c1f

Scoped source review of retry changes relative51e0cfb6 found no actionable defect. Proxy thrown errors get at most three retries (250/500/1000ms) in the same flight; the running slot, immutable source and subscriber/drain remain owned throughout backoff. Task.sleep is cancellable, thrown waits terminate, and explicit post-wait cancellation check also prevents a deliberately non-cancellable injected clock from issuing stale requests. Pending responses still wait on the already-installed buffered event subscription. Original errors remain terminal without retries or fallback. Retry budget does not reset after pending, so failures cannot create an infinite loop.

Deterministic tests verify retained slot, eventual success, four-call exhaustion, supersession/drain ordering, thrown wait termination, Original nonretry and event-driven pending path. Existing test uses immediate injected waits to avoid unrelated elapsed-time dependence. These tests are unrun by reviewer.

After exhaustion loader honestly terminates without pixels or phantom pending; it does not introduce per-image error UI. A persistent queue backlog lasting beyond the total1.75s backoff can therefore still leave a placeholder until explicit rerequest. This is a bounded recovery policy, not a guarantee every overload resolves. Existing cached-Library notice discloses possible missing thumbnails. Broader failure UI is outside this narrow correction. Native/runtime scrolling qualification remains necessary.
