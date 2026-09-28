# Phase 10 source checkpoint (UNRUN)

This is an isolated Core Graphics destination-context experiment. It is not product source and does not change ImageIO decode options, source files, gain maps, profiles, render color space, sampling, acceptance limits, or prior evidence.

`probe/imageio-probe-context.m` is copied from the accepted phase8 `imageio-probe-geometry.m`. Its only behavior delta is a numeric `PROBE_TARGET_HEADROOM` input replacing phase8's zero-only setter switch. If unset, it preserves the default context target and does not call the setter. If set, it parses a finite positive float, calls `CGContextSetEDRTargetHeadroom`, and records requested value, setter result, initial context value, and resulting getter value. The call remains on the same destination bitmap context before `CGContextDrawImage`; provider bytes and returned ICC are copied from the already-decoded `CGImage` before that draw. The ordinary source-property inspection remains after both SDR and HDR decode/draw operations.

`probe/run_context_headroom.py` runs each exact retained 80x16 input in separate output directories/processes at default, 8, and 16. It verifies current fixture SHA-256 against the phase8 manifest; compares the new default provider, ICC, image headroom, and drawn-float hash against phase8 before interpreting target runs; and checks the setter/getter plus provider/ICC/headroom invariants for target pairs. It saves raw outputs, JSON, warnings, stderr, command/environment, and direct exits under a caller-specified external run directory. It records pixel changes without asserting a desired peak. `--binary` requires the compiled executable to stay on the validation volume rather than in this source checkpoint.

Exact fixtures are copied from phase9 and independently required to match the phase8 hashes:

- `A80.jpg`: `bb08f44df33f9426999edc116adc9fecd61a4f70a9fc351318071c155d2c413b`
- `split80.jpg`: `eaeb9524ddfd2cee2d98c34512103ee7e06819471223de9caf1a8642c69c8968`
- `uniform80.jpg`: `61824f8eb85beb703d17c486e4c146c755ab6b6726213d4502aa6bf3b867f351`

This source checkpoint is UNRUN. Its only intended native process matrix is 3 inputs × 3 context states; all earlier phases and the original ImageIO acceptance failure remain unchanged.
