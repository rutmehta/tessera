# Corrected History qualification — preserved second RED

Exactcd0b850d gate01 exited1,66tests/3assertions in2.389s after freshReleasebuild178.53s; source/head/FFI/fixture identical. Only testActualInspectorAXActionWritesExistingPreferenceAndRestoresOnRecreation fails. Other5History tests and all15keyboard tests pass within original combined order. Four History downcast warnings gone; preexisting unrelated test warnings retained.

Observed: increase leaves preference168 rather than192; recreated readout0pt rather than192pt; containment reports controls outside host{{0,0},{0,0}}. This is actual zero-sized hosted-view evidence, not a guessed timing fix.

Source: DocumentHistoryHeightControlTests.swift193–205 installs NSHostingController into an initially288×848 window but does not explicitly size hosting view/reset content size after assignment. A GeometryReader-only root need not impose a nonzero intrinsic window size. DocumentInspector.swift128+ reads column.size.height; at0 the budget clamps height0 and both step buttons are disabled. The original ShellHarness.settle explicitly resets frame after assignment; unrelated DocumentVectorVerifyFixesTests.swift255 directly sets host.frame.

Bounded correction for B: explicit intended host/window sizing after attaching controller, layout/event-loop settlement, and positive/intended host bounds plus enabled increase assertions BEFORE interaction. Preserve all expected192/default preference checks, recreated readout, containment and singleton restoration. Do not change product clamp, add arbitrary delays or relax values to hide zero-size fixture. No source mutation or retry performed by reviewer. Remaining gates stopped. Root notified; lane idle pending correction coordination.
