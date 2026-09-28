# RAW pure admission qualification c595dad2

Compile01 and exact focused02 passed (20/20); preserved original full03 failed on absent intended sample.dng before that test's behavior assertions. No source/test repair was made for that setup failure.

Versioned fixture retry: full04 direct0, 116 passed, 0 failed, 2 ignored across 24 summaries (includes zero-test doc summary). All 20 named admission tests passed again within full suite, and previously blocked cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back passed. Full04 elapsed 50.49s.

Strict05 all-target Release Clippy -D warnings direct0; existing native libraw dependency warnings retained in raw log. Fmt06 cargo fmt --all -- --check direct0. All three before/after snapshots exactly equal: Git source/HEAD/status, external fixture discovery, runner/oracles/baseline hashes. jobs2/deployment15/shared BetterSSD target preserved. No test weakening, new source edit, app launch or benchmark.

Fixture is the fetch.sh Leica M9 DNG, 36,433,920 bytes, SHA914f27df1ab095446c5db058703719f4a3749adedbfb9f353a3e85fa4f8f9b58. Source copy and unchanged originals documented in fixture-provenance.json. Only this RAW is present; optional default fixture selection exercises DNG instead of historical Sony. This is not five-family qualification. Optional model/cache absence and ignored tests remain noncoverage.

This qualifies only the reviewed private pure predicates and checked arithmetic. No pixel renderer, normalization, authenticated decoded provenance, resource reservation/cap policy, ICC/environment authority or public integration is implemented or accepted here. Original b950 RED and missing-fixture03 remain preserved. Runtime lane released after06.
