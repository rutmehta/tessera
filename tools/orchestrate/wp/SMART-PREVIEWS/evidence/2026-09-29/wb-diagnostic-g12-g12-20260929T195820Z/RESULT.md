# WB live diagnostic — G12 runtime result (g12-20260929T195820Z)

Heads that ran include the post-review D0 hardening commit (reviewed afterwards: driver/interpretation only, adds Inconclusive paths, no product change). Single invocation, automatic Metal selection (never forced), one epoch per process, read-only Sony ARW copy (sha256 bf4c6d21…, blake3 df562403… as recorded in process.json). Baseline claude/wb-diagnostic-d-baseline 7b16217e (5f31f148 + diagnostic), candidate claude/wb-diagnostic-d-candidate 90ae129e (7a3ec9bf + diagnostic). All four processes rc 0; protocol-validated; inconclusive=[], contradictions=[], invariant_violations=[] in every interpretation.json. Source freezes equal before/after.

| | Baseline | Candidate |
|---|---|---|
| Reopen | 3 Metal calibration transactions (CPU unobserved +3), no cache | Decision-cache TTL hit (1 hit, 0 measurements, 0 publications, identical key); 0 calibration |
| First Daylight edit | Detail=Hit (key requested by calibration iteration 2) | Detail=Miss, PaddedWb=Miss, TileWb=Miss |
| Repeat Daylight | Detail=Hit | Detail=Hit |
| Custom 6500K/+10 | all Miss | all Miss |
| Route (all edits) | Coarse | Coarse |

Same in SDR and EDR. k=0 superseded jobs every phase. Mutex box size not measured.

Interpretation (attribution only; NO timing/performance/remedy claim): the baseline's reopen-time calibration renders prime the exact resident Detail cache entry that the first Daylight white-balance edit needs. The candidate's decision cache skips calibration, so that priming never occurs and the first WB edit misses at every instrumented bucket (Detail, padded WB, all 35 tile-WB lookups) and re-requests them. The candidate's missed key equals one its own first-open calibration had requested, so the miss reflects the fresh cache after reopen, not a different key. This is an exact-key cache-state difference consistent with the first-edit regression; G12 does not establish that it causes or fully explains the timing difference (rev7 §8, §9.5). Pixel dumps (.rgb32f) retained on betterSSD only; hashes in pixel-and-binary-sha256.txt.

Independent verification: VERIFIED-WITH-CAVEATS (all facts recomputed from raw phase records by an independent script); corrections above applied.
