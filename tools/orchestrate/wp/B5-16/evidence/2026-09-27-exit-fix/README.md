# Targeted self-test repair

App harness fixes1f2f0ea; runner startup fix72d8756. Channels and Text now exit
on completion; Transform prerequisites increment its failure counter. Removed
Transform's --new-document argument: the original run selected the blank startup
2400x1600 document, while its intended card is1600x1000. Original screenshot and
log show that mismatch. A fresh Transform run must establish whether this repair
resolves the failure; do not infer success from the source change.

First packaging attempt compiled but failed provenance because HEAD advanced
during compilation. It was not used for tests. The retained retry held HEAD at
72d8756 and passed signing and provenance. Eight runner tests passed.
Channels rerun PASS, zero failure summary and actual process exit0. Text and
Transform are still in progress at this checkpoint; original failed runs remain.
