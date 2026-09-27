# UX-02a — navigable Review (in progress)

Scope: session-local destination/navigation only. Queue relaunch persistence is
not implemented; UX-02b remains required for the full UX-02 goal. No Rust, FFI,
Document UI or foreground activation changes are part of this slice.

## Verified before implementation

- Original release regression compiled in23.29s and ran1test with4expected
  assertion failures/0unexpected: opening the queued out-of-filter target through
  the former Review `showInLoupe` path changed source, visible IDs, selection and
  focus. Original log: `swift-navigation-red.log`.
- Ownership dependency's independent gate is recorded under
  `../UX-02-ownership/RESULTS.md`; it does not validate this navigation draft.

## Current implementation and pending verification

The draft adds Review navigation/cursor/draft state, a virtualized list, Current
preview and per-photo inspector actions, explicit target editing without Library
filter changes, stable Library return state/native anchor, and keyboard/undo
routing. The legacy People/Tether inspection helper retains Library Loupe.

New tests cover filtered real-queue round trips, matching Develop session identity,
Accept-and-next success/foreign rejection, empty/all-reviewed reachability, draft
state, keyboard priority, native resize restoration and legacy inspection. A real
JPEG scripted queue plus a removed indexed file drives the planned24background
layout cases (empty/populated-long-name/failed, four sizes, two appearances).

No navigation GREEN/full-suite/layout result is claimed yet. The targeted future
session-open mutation guard is being integrated from the ownership agent; no
relaunch persistence or before/after pixel baseline is claimed. Existing UX04
photo-overlay/Mask Components polish remains separate.
