# Offline Library source review

Reviewed source-only against baseline `81a6cc53a18e2ea06d1e461a5f2cb6b3ea2c5f8a`, using the frozen patch and staged sources. No patch applied, builds run, or shared sources edited.

## Finding

**P2 — Fresh read-only sessions cannot query frames for a person.** `crates/tessera-ffi/src/assist.rs:1430–1432` in the staged result calls `refresh_people_job(false)` whenever the people cache is empty. New offline sessions start with that cache empty, and the newly added `require_writable` at line 1043 rejects this refresh. Thus the documented read-only per-person filter returns a read-only error until an unrelated `people(false)` call has populated the cache. Use the declared-session exclusion already implemented in `Inner::people`, or populate through that cached reader, before querying `person_frames`. Add a test invoking `frames_with_person` directly on a fresh declared session, without first calling `people`.

## Accepted bounded behavior

Opening, listing, grouping, and FILE sync operate from the catalog and local declaration files. Original folder canonicalization/existence checks, sidecar selection reads, history-derived status, and dHash generation are bypassed in those paths. Admission checks bounded journal structure/ownership and regular local asset presence; this intentionally does not validate pixel content. Image-open validation remains required. Folder membership uses path components, requires absolute paths and rejects parent traversal. FILE updates retain missing original members while moves outside scope/removal still eject them; the declaration allowlist is a documented snapshot.

Core selection/history/library/basket mutations and FFI people mutations/refits are guarded. No actual original-write bypass found. The existing FFI selection-admission wrapper still probes original paths before reaching the core read-only rejection for attempted selection mutations; this does not violate the narrowly documented opening/listing/grouping/sync guarantee, but early policy rejection would avoid unnecessary disconnected-volume access and misleading reconnect errors. Do not describe all API calls as entirely free of original filesystem I/O.

Prepared tests cover declaration vs validation, membership retention, and mutation refusal. They were not compiled or executed by this review. Approval of the bounded API is pending the P2 cached-person filter correction and runtime gates; this review does not establish complete application offline startup or UI integration.

Patch SHA-256: `34c5856eed4975d31d3874aab2ba394e5c02a3ec2d7a999ec9de30d4a5547c1d`.

## Revised patch disposition

Re-reviewed the revised patch rebased onto `e1eca7ba9c6a61239777eb9ad637a5d439461410`. The P2 is resolved: `frames_with_person` excludes declared read-only sessions from its initial refresh, and the new regression invokes both ordinary and eyes-closed filters before `people(false)`. The workflow additions retain the clean-offline recipe regression introduced by e1eca7ba. No remaining actionable defect in this scoped source review. Approved as a bounded source-level API change, subject to the implementer’s compilation/runtime gates. No builds run here. Prior limits, including declaration-only admission and original-path probes during rejected selection mutation attempts, still apply.

Revised patch SHA-256: `cdd2aba96c2361b16092bc34e055280b7df35d549522b1f5e1b6424dae9ceda0`.
