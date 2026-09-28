# History numeric font token — source-only handoff

Request680a8220-8d1d-46bb-af94-10b67d5e3180 validated and accepted before edits. Separate branch codex/b5-16-history-theme-token from frozen4aaed30a. Product commit7d9199afdece8ea40e55d7e16ec50e927e02dd5f.

Exactly one product line changes: DocumentHistoryHeightControl.swift57 now assigns Theme.NSFonts.labelNumeric. Existing token in App/Theme.swift225 is 12pt regular monospaced digits, the requested semantic numeric label font. This is not a claim of size equivalence to NSFont.smallSystemFontSize. Control132/readout48 widths, AX/keyboard behavior, preferences, layout code and all tests/lint remain unchanged. No lint exemption.

A mainb1567cd9 preserves history-7732a03e-theme-red: full720 XCTest/1existing opt-in skip/1ThemeLint failure plus5 SwiftTesting passed; strict not run. A reports combined66, keyboard15, layout4, adjacent84 and actualSony workflows passed on that prior candidate. Earlier65/22 and66/3 failures remain preserved. Those results do not validate this source change or establish external AX/GUI acceptance.

B performed source inspection and git diff --check only. All affected tests UNRUN on B. A to compose exact source, rerun ThemeLint/History and affected layout, then full/strict and actual AX/keyboard GUI qualification, including readout fit at compact sizes after adopting12pt token. A owns runtime and main merges. No B build/test/app/GPU/benchmark/heartbeat/writer changes. Companion manifest includes exact product/test/DocumentView hashes.
