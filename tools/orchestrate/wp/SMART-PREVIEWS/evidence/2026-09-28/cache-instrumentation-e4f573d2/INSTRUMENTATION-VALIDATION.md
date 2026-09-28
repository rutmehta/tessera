# Instrumentation-only qualification

Final clean commit `e4f573d2ed6afe7a9e7ca52b623e8dc763b46295`. No decision cache integrated.

Original attempt01 remains instrumentation RED (control absent; actual backend mismatch). Reviewed245c9095 then passed explicit compile02 and uncached controls03; unchanged six contracts04 gave five initial missing-entry failures plus one passing existing cold validation control. Strict05 passed. Formatting06 failed and is retained.

Only rustfmt changed five source files in e4f573d2; `formatting-only.patch` and `FORMATTING-CHECKPOINT.json` record it. Final exact bytes: compile07 direct0; uncached control08 direct0/1pass/0ignored; unchanged contracts09 direct101/1pass/5fail/0ignored; strict10 direct0; workspace fmt11 direct0. All phase freezes equal (8,646 source inputs each), original Sony fixture hash unchanged. Compiler/vendor warnings remain in complete logs.

All five new cache groups reached the initial entries assertion (actual0 versus expected1), after actual backend and measurement assertions. This establishes missing initial storage/publication, not missing reuse: later assertions remain unexecuted. Existing cold validation group passed separately. No original-contract acceptance or performance claim.

New uncached control materializes actual CPU/Metal backends in SDR/HDR using controlled samples, opens twice, observes two successful validations/measurements/normalized candidate capability calls, fresh released Renderers, and zero lookup/hit/publication/entries. It checks overrides and error/device fallback too. This is not timed GPU pixel-dispatch evidence, actual in-flight device loss, external resolver verification or cache eligibility qualification.

Exact commands/environment and source/fixture/runner hashes are under each attempt. Original pinned runner is unchanged; instrumentation runners pinned separately to245c9095/e4f573d2. Both scrub inherited TESSERA flags and use explicit Sonyfixture, BetterSSD target, jobs2 and deployment15.0. Raw attempts02–11 retained; run-instrumentation*.py and INSTRUMENTATION-FINAL.json bind the final report.

Runtime lane released after11; no additional implementation authorized or performed. When cache wiring starts, validation observation must preserve cumulative lookup/hit/publication counters rather than reset them.
