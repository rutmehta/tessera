# History test-isolation correction review

Reviewed12e9f1ff/b7687116 frome2c3cd03, all three handoff hashes verified. No blocking source issue identified. Source-approved for composition and runtime, not a passing runtime/AX claim.

Only tests/handoff change. Product control, DocumentView and KeyRouter unchanged. Replacing unsupported direct AX role/Bool assumptions with exact callback sequences and native displayed N pt is supported by the preserved plain-AppKit probe. Missing/double actions still fail; clamps, state values, preference writes/recreation, callback replacement, teardown and containment remain asserted.

Direct actual DocumentInspector hosting removes the unrelated DocumentView singleton attachments/outline activity from the preference test. Existing whole-shell layout tests remain required. Capturing five prior owners strongly, asserting identities before teardown restoration, and preserving absent preference values avoids blindly clearing preexisting state. Added sixth regression restores explicitly nonnil owner identities. Local windows are removed/closed in defers before tearDown and activation policy is restored. No product shared-owner behavior is modified to make tests pass.

Runtime must rerun the ORIGINAL combined filter/order, separately run keyboard-alone against the same binary, then layout/full/strict. Narrow hosting does not prove whole-shell isolation, external AX role, focus traversal or speech. Existing RED65tests/22assertions and profileless GUI relaunch audit remain intact. No compilation or GUI performed for this review.
