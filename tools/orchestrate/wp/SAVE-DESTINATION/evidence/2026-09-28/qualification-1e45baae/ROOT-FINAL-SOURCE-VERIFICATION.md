# Independent final Save As qualification verification

Verified immutable candidate `1e45baaead70b1bcd208afba800bcacd92213ee8` against Git blobs and recorded evidence. No builds, applications, GPU work, source mutations, or merges performed. Only requested verification artifacts written here. Detailed recomputed results: ROOT-FINAL-SOURCE-VERIFICATION.json.

## Verified automated qualification

All **7,804 tracked Git blob SHA-256 values** match each final adjacent11/full12/strict13 source snapshot. Their before/after HEAD is the exact candidate; source, ignored FFI artifacts and source-fixture snapshots are unchanged. Current checkout inputs also match the final snapshot. Focused10-03 source bytes match the same immutable candidate although that gate preceded the commit.

Native01–08 all directly exited0 with unchanged per-run inputs. Compared every captured source hash to final Git: the ONLY later differences are the two generated Swift/header files and DocumentSaveDestinationCommitTests.swift. No Rust/Cargo/native product differences. Native logs confirm4 publication,5 public Save As,1 roundtrip,1 PSD,11 document+1ignored,193 FFI+1ignored; strict all-target Clippy and fmt exited0. Generation09's only tracked changes are its authorized header/Swift output.

Swift focused10-03 directly exited0 with36/0; adjacent11 directly exited0 with48/0. Full12 directly exited0 with703 XCTest/1skip/0fail and5 Swift Testing passes. Both actual Sony workflow successes occur exactly once in the unchanged raw full log:
- cached thumbnail after original disconnect:5.162s;
- offline Library/render/save/reopen/reconnect:5.235s.

Independently parsed full-line Darwin XCTest class/method success records, rather than relying on substring labels or the summary. They exactly match required-workflow-revalidation.json; its log hash also matches the raw file (`382388ba46be5f923f87bae0b8ee420de3e69d0b444256b4c7e35d2f995e6398`). The original wrapper's postcheck failure is valid retained evidence of a parser bug, not a failed native subprocess: run.py writes the direct subprocess exit before postchecks. Corrected parser accepts Darwin bracket notation and still requires the exact requested method plus passed marker. No rerun or log rewrite was needed to establish these two explicit successes.

Strict13 directly exited0 using complete strict concurrency and warnings-as-errors; log confirms Tessera Release product build151.08s. Initial focused10-01 and10-02 remain failed (2 and3 assertions respectively); final test-only repair is not retroactively attributed to them. GUI qualification remains separate.

## Artifact and fixture identity

Current archive and all three generated artifacts match generation09 and final13 recorded SHA-256:
- libtessera_ffi.a: `5b7e5eba7909c4641b4108a3ed3c92f7cd6d204f1a792d0e3660a12c7d162594`
- CTesseraFFI.h: `66598c0c7f27f28fd8605f1b41ad7521b20ec054c902d3d57cf15f5fd621fd45`
- TesseraFFI.swift: `daf7b29d4cb29838fc18eb37a5f28b1bfd8eff7c892babe0d0a7608f4e110898`
- CTesseraFFI.modulemap: `efda206de8cf8eb6c092c29fd32f286b9a46d9d7c4c150e6e9b66941dc43d6bd`

Rehashed source Sony fixture remains `bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8`. Earlier independent generated review confirms all434 existing API checksums preserved and native checked-save checksum46144; no stale Smart Preview bindings were substituted.

## Acceptance gaps

No remaining automated source/evidence blocker found in the inspected scope. This does not accept real Save As collision/Replace GUI behavior: storage owns that separate runtime check. Existing opt-in20k library skip, ShellLayout expected-failure coverage, ignored large recomposite benchmark and ignored Engine performance qualification are not newly qualified here. No new physical-presentation, memory, cross-camera or broad B5-16 interaction claim.

B5-16 reconciliation remains preserved at /tmp/tessera-b516-current-review.md and companion JSON; its outstanding layout/editor source and interactive work are separate from this candidate.
