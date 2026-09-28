# Checked Save As qualification

Final qualification branch `codex/save-destination-a-qualification`, HEAD `1e45baaead70b1bcd208afba800bcacd92213ee8`, based combined `5f34221d` (accepted GPU main + Save As source). No main merge or app launch performed. Compiler gates complete; runtime lane retained for GUI preparation under root instruction.

## Results

- Native Release01 publication4 passed;02 public Save As5;03 roundtrip1;04 PSD1;05 full document11 passed/1 ignored;06 full FFI193 passed/1 ignored.
-07 strict Release Clippy (`--all-targets -- -D warnings`) and08 fmt passed.
-09 coherent native archive + bindings generation passed. Exact additive delta: header11 lines, Swift164 lines, checked method/two enums/checksum46144. Independent reviewer confirmed all434 existing checksums and Smart Preview APIs preserved. Exact generated whitespace retained; diagnostic diff-check recorded.
-10 focused Swift: initial01 failed two assertions in one test;02 ineffective directory normalization failed three assertions in two tests;03 passed36/36 after reviewed test-only repair. Production unchanged. Foundation enumeration emits/private/var but fixture URLs use/var; comparing known filenames within the same directory fixes the oracle. Positive saved `owned` and retained `ours` byte assertions added; existing counts/foreign/unowned/sentinel checks retained. Both RED attempts and inode diagnostics preserved.
-11 adjacent Swift48 passed.
-12 full Swift directexit0:703 XCTest cases,1 skipped,0 failures;5 Swift Testing cases passed. Existing ShellLayout expected failure accepted by its matcher; opt-in20k library test skipped. Actual Sony cached-thumbnail workflow passed5.162s and offline Library/render/save/reopen/reconnect passed5.235s.
- Full runner initially returned1 after the successful subprocess because its postcheck expected dotted class.method rather than Darwin XCTest class method notation. Original log/exit/freeze remain unchanged. Both exact successful workflow lines revalidated with corrected parser in required-workflow-revalidation.json; no unnecessary full rerun.
-13 strict Swift Release product build with complete strict concurrency and warnings-as-errors passed151.08s.

Native excluded workloads: `bench_interactive_recomposite_100_layers_20mp`; `develop::preview_qualification::engine_iosurface_matched_viewport_qualification`. These were not executed by this qualification.

## Provenance and limits

All gates use jobs2, deployment15, explicit shared Cargo target and separate Swift scratch. All test gates use the preserved Sony fixture. Source/HEAD/archive/fixture freeze checks passed per gate except the authorized generation09 artifact changes. Focused03 preceded commit; adjacent/full/strict ran at immutable final HEAD. Original inherited ignored archive/bindings remain in inherited-ffi. Final artifact hashes are in13-swift-strict-01/after.json; full-suite artifact hashes remain separately in12-swift-full-01/after.json (strict rebuild may differ).

No Save As GUI collision/Replace acceptance yet. Await root source/evidence review before that separate bounded step. No user documents changed.
