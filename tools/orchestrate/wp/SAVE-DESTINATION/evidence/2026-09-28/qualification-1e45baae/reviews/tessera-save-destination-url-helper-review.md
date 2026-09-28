# Save destination test URL helper — source review

Approved proposed one-line test-only change: return `url.resolvingSymlinksInPath()` from `directory()` after creating that directory. No product change or assertion deletion is needed.

The retained diagnostic `url-diagnostic-with-identities.log` proves the destination `/var/.../out.tessera-doc` and enumerated `/private/var/.../out.tessera-doc` are inode 73444986, while direct URL equality is false. The distinct stage is inode 73444987. This supports a fixture URL representation defect rather than a failed save/cleanup contract.

Inspected both direct URL exclusion predicates in the current file: line186 `filter { $0 != url }` and line202 `first { $0 != retained }`. Both derive expected URLs from directory(), so resolving the existing fixture root consistently fixes both. The latter was order-dependent under aliases because even the retained original stage incorrectly satisfied the predicate; canonical root construction restores exclusion of that entry. Current file has two direct URL exclusion predicates, not three. Other directory assertions compare relative names or counts and remain unchanged.

Resolution happens only on the already-created disposable root. It does not resolve the deliberately-created destination symlink, alter symlink rejection coverage, change beforeCommit hooks, or weaken stage count, sentinel contents, inode identity, dirty-marker, and destination absence assertions. Teardown may keep the captured original alias because it names the same fixture root. This is source approval; the lane owner must rerun the affected tests to establish runtime success. No compilation or app/GPU execution performed by this reviewer.

Before SHA-256: `d6cfbc1f50b7df036767af75f374fa48338661a1d41b3518e8ed04a2bbe324ae`

Expected one-line candidate SHA-256: `5bd6a4888f9d2eb4a12aea563fef7f38e17d29784af66c3bdb495546bd54d40f`

## Superseded initial approval; revised oracle review

The first proposed helper fix did not work at runtime. Focused10-02 retained three assertion failures in two tests, and url-diagnostic-canonical.log shows Foundation still returned /var for the resolved fixture URL while enumerating /private/var. My prior source approval incorrectly assumed one-sided resolvingSymlinksInPath would normalize these representations; it was insufficient and is superseded. Retain both failed runs and diagnostic evidence.

Source-approved revised exact scope:

1. Revert directory() to return url.
2. In testSuccessfulRenameDoesNotCleanupReusedOldStageName, exclude entries by `$0.lastPathComponent != url.lastPathComponent`. Preserve `other.count == 1` and other entry bytes == unowned. Add explicit destination bytes == owned.
3. In testReplacedStagingNameIsNeitherPublishedNorDeleted, select `$0.lastPathComponent != retained.lastPathComponent`. Preserve total count == 2, missing destination, thrown write, and selected entry bytes == foreign. Add explicit retained bytes == ours.

This does not treat filenames as general filesystem identity. Each enumeration is nonrecursive and restricted to the exact disposable directory from which its known target/retained URL was constructed. Distinct directory entries cannot share the same exact filename there. In the first case, asserting destination bytes exists and equals owned plus one non-destination entry with unowned bytes establishes the intended two-file result. In the second case, asserting retained bytes equals ours, two total entries, absent destination, and the remaining filename's foreign bytes establishes the intended retained/foreign result independently of enumeration order. No path normalization, symlink-target identity, or parent string equality is assumed. Deliberate symlink safety tests and all inode/sentinel/stage assertions elsewhere remain unchanged.

No product changes or builds performed by this reviewer. This is revised source approval only; storage's next focused run supplies runtime evidence.
