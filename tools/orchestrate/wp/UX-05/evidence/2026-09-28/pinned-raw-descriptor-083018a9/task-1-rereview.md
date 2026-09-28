# Task 1 scoped re-review — 12b354fd

Reviewed only the supplied fix diff `771a508f1d43a5ee7ad815b69987829d2e8062ac` → `12b354fd35effa5972d5743fbe353aee01abfb6e`, the amended brief, updated task report and coordinator ruling in progress.md. No runtime, builds, source modifications or subagents. This review artifact is the only file written by the reviewer.

## Verdicts

**Spec compliance: addressed; source cleared for broader engine-api gates.** Findings1–4 now have meaningful targeted controls, and the pure descriptor scope is preserved. **Code quality: addressed; source cleared for the assigned formatting/strict gate.** Finding5 and all three inexpensive notes are resolved. No new semantic regression or acceptance blocker found in this fix diff. Actual broad tests/format/strict outcomes remain pending; this is not whole-branch acceptance.

| Prior item | Verdict | Evidence in the fix |
| --- | --- | --- |
| 1. Nondefault current settings/hash versus history | Addressed | `current_nondefault_settings_determine_recipe_hash_and_input_identity` uses exposure0.625 with a valid stale default history, checks exact bytes and stored RecipeHash against the constructed current Recipe, then changes exposure and requires changed identity. Existing opaque malformed-history control remains. |
| 2. Nondiscriminating nested duplicate | Addressed | The nested case now duplicates recognized exposure and escaped-equivalent exposure, asserting duplicate-error text. A separate outer descriptor duplicate control also asserts that error. Existing valid payload tests provide positives. Unknown-field rejection can no longer satisfy the nested duplicate test accidentally. |
| 3. Arrays/enums/legacy profiles/numeric recursion | Addressed | Added valid flattened Sky/ModelRef component, then component/model unknown members and unknown variant negatives. Camera and Database lens bare-string and canonical-object positives execute, followed by profile-extension rejection. Negative exposure and nested ColorRange sample f32 overflow augment the positive exposure overflow case. |
| 4. Missing headers/defaults/identity | Addressed | Tests now cover missing process/family/revision, missing/null owner, schema types, empty settings object positive and nonobject negative, unsupported native revisions, declared-length change, suffix case equivalence and suffix change. |
| 5. Clone on Copy | Addressed | `from_json` copies `wire.decoder_route` directly. |
| Note: duplicate numeric traversal | Addressed | Removed check_numeric_integrity; recursive check_shape retains Number→Null rejection. Array-length/shape checks remain, and both signs plus nested overflow tests pass. |
| Note: hardcode admitted process in hash | Addressed | Temporary Recipe uses explicit ProcessFamily::Native/revision2, independent of future NATIVE_CURRENT changes. |
| Note: contract invariant placement | Addressed | Invariant19 moved beside invariants1–18. |

The authorized fifth path is limited to `tests/m532_channels.rs`'s exact version assertion changing1.6.0→1.7.0. `src/lib.rs` changes CONTRACT_VERSION correspondingly; CONTRACTS records the additive revision and no recipe/native-process revision change. The coordinator's portability/client-version cost ruling is explicit in progress.md. No additional product surface was added.

The newly added test fixture uses a Default-then-field assignment and a one-element loop; the upcoming strict gate may request mechanical lint cleanup. These do not change the semantic verdict or justify speculative test execution by this reviewer. Preserve any actual gate diagnostics if they arise.

## Independent committed evidence audit

Saved prefix: `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor/review-fix-committed`.

- Durable returncode file is0. Raw log reports exactly20 distinct named tests passed,0 failed/ignored. Log SHA256 `1cb85c35b77a08fdad9d4672627fc841c3e7b777ca3798eb0afa7d4a8d049864`. Previous13 and interim tests are not additive.
- Metadata records `cargo test -p engine-api --test pinned_raw --release --jobs 2`, the export-integration checkout and explicit BetterSSD Cargo target; rustc1.98.1/cargo1.98.1.
- Before/after snapshots compare equal at exact clean head12b354fd, with empty status/diff hashes and aggregate tracked-tree hash `33d6a78cb2625b47b81151d013cabfef52345bffd642f451555207cbc2af25b8`.
- Independently compared all five assigned-file hashes against exact12b Git blobs; zero mismatches. This is five-file Git verification plus aggregate before/after equality, not a claim of independently rehashing every Git blob.

Updated report corrects the initial green-focused-3 count to6 and retains the early untracked-source/runner-version provenance limitations. It separately records the expanded precommit fixture error (changing Sky to Sky while expecting rejection), its correction to future_kind, and committed20 success. The source-bound committed run is the relevant current gate; historical drafts are not upgraded to exact-source evidence.

Next step: root may grant the planned broader engine-api tests, crate formatting and strict Clippy to the implementation owner. Decoder/capture/render/FFI/UI work remains outside this task, and final integration still depends on those assigned gates and evidence review.
