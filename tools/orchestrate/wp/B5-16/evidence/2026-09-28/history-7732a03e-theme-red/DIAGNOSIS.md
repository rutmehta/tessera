# Qualification checkpoint — theme-token failure

Candidate7732a03eff48e029982d46ffda56148a1047744f unchanged throughout. Combined66pass, keyboard-alone15pass with executable hashes unchanged, layout4pass80.293s, adjacent84pass. Full720XCTest with1expected20k opt-in skip and1failure;5SwiftTesting pass. Both actualSony workflows pass4.886s/5.297s. Full directexit1; all source/head/archive/fixture freezes true. Strict not run.

Only failing assertion is ThemeLintTests.testViewsUseThemeTokensOnly at testline55: DocumentHistoryHeightControl.swift57 uses ad-hoc NSFont construction. Existing Theme.NSFonts.labelNumeric is monospaced digits12pt regular and fits intended readout; captionNumeric11pt is also available if design chooses smaller. B should replace call with chosen existing token, preserve lint unchanged, and qualify focused ThemeLint+History/layout then full/strict exact final source. Do not hide with lint:allow or alter expectations. No local source change or retry made; root notified for B-owned correction.

Previous two RED attempts and diagnosed fixture fixes are retained in their separate evidence directories. Current behavioral History/keyboard success does not imply external AX/GUI acceptance. Prior profileless relaunch exception remains recorded.
