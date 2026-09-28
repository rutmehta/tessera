# Runner admission follow-up (source-only, UNRUN)

Request cf3510b8-7ad7-4ce8-a5e5-038fdc3f4770, 2026-09-28.
Branch: codex/b5-16-runner-admission.
Frozen base: ddb2810196d5492c9258840af9a431e706d9318f. The inspector candidate
codex/b5-16-current-main remains at that exact commit, unchanged for A validation.
No rebase to later Save As/main. No app/Swift/Rust/generated changes.

- Test commit: 1139443d1b194ed809c7e5ad77a28cb615f9f100.
- Product commit: be7abbb5b222c86143fd6b962f4e3c5522b9fce8.
- All twelve Python test methods remain UNRUN on this candidate. A's reported
  seven earlier passing tests do not qualify this increment. No B Python runner,
  tests, compile, apps, GPU, benchmarks or heartbeat changes were performed.
  The required Git-mailbox Python utility was used solely for coordination.

## Established policy and correction

Transform fixture correction is restored from
72d8756a643c22ca1bab2ecf9822f696c3df4f07: timeout1800 with exactly
`['--transform-selftest=@OUT@']`, no `--new-document`. Transform opens its own
card; a delayed blank startup document previously replaced its selection while
waiting for a view. Regression now pins the exact timeout/arguments, not merely
suite inclusion.

Resource admission policy is restored from
131af17d735c648bb5d335e9f79b7b6b2b063c1b, specifically selftest_runner.py and
`tools/orchestrate/wp/B5-16/RESOURCE-AUDIT.md` at that commit (also preserved on
wp/B5-16). Its existing marker is `~/.local/state/tessera-resource-hold.json`.
The file is still present on B; it was read, not changed or deleted. Presence
blocks admission regardless of JSON contents. No second marker, policy schema,
environment bypass, override flag or authority to lift the hold was introduced.

The same guard runs before main's path/fixture/scratch work, before each dispatch,
at direct run_test entry, and after staging immediately before Popen. A hold
observed between cases prevents dispatch of the next case; one observed during
staging prevents the child. This is admission checking, not an atomic launch lock
or a monitor/termination policy for an already-running child. No claim is made
that a marker created after the final check can revoke an already-admitted launch.

## Authored negative coverage — UNRUN

Five added tests cover absent/present/malformed marker semantics, default and
explicit Transform main entry with no run_test/Popen/capture/scratch creation,
direct run_test blocked before staging, a marker introduced by the staging hook
with no child, and a hold after the first mocked case stopping later dispatch.
The real-child fixture and existing mocked-suite test isolate Path.home to their
own empty temporary home rather than consulting or altering the real user's hold.
The seven original tests remain, including failure-summary and child-ownership
checks; the existing Transform test now pins the corrected arguments.

`--nonactivating` still only requests accessory launch. Individual app self-tests
can raise windows; no desktop isolation is inferred. No actual app selftest is
authorized. A owns any later Python/fake-child execution, compiler/runtime and
main gates. Broad app acceptance, Transform completion and inspector interactions
remain separate and UNRUN. Historical failures are preserved.

## Source checks and exact SHA256

`git diff --check ddb28101` passed. Source review only, not Python execution.
Only the two files below plus this handoff differ from frozen inspector source.

```
79cddba47552e92670d16874bbdf99f885d1703b92307c4a55533c5ab44af6a5  tools/orchestrate/wp/B5-16/selftest_runner.py
17ad31549b639eae3cdf17eb8d873adcce5f7587ff8af4736eb3207406855a53  tools/orchestrate/wp/B5-16/test_selftest_runner.py
```
