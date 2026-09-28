# PSD copy error precedence correction — fbd39d94

2026-09-27. Exact B chat target and expiry validated; accepted receipt published
before source edits. Base candidate 7240948 was reviewed, not passed.

## Correction

The previous run finalizer looked at the cancellation flag after run_inner
returned, so a genuine error could become Cancelled. The operation now carries
CopyResult<T> with CopyError::Cancelled or CopyError::Failed(BridgeError).
Finalization releases admission and maps only that typed result; it never
reclassifies a failure from a later flag. Running-drain admission, single-use
handles and pre-persist commit ordering are unchanged.

Explicit checkpoints and rejected cancellation/close commit admission produce
the cancellation variant. Validation/IO/encoding/ownership failures preserve
Failure. EngineError::Cancelled is matched as a variant before BridgeError can
erase its type. Native stack's internal evaluator helper now preserves
EngineResult; its existing legacy wrapper still converts to BridgeError.
The checked raster conversion helper carries its caller's error type so a row
checkpoint cannot lose its cancellation classification. Copy-only IO likewise
carries the typed error through temp-file cleanup.

The native adapter also had flag-based error inference. Genuine specialized
adapter failures now return before a later cancellation check. Legacy menu-filter
BridgeErrors are conservatively kept as failures, never inferred as cancellation
from a concurrent flag. Important limitation: those older menu/effect routines
already erased cancellation into BridgeError; their ambiguous cancellation error
may therefore be surfaced as Failure. No error-string matching is used. A future
typed legacy-effect error path is needed to distinguish that case reliably.
This correction does not claim whole-PSD cancellation or bounded latency.

After acquiring the backend lock, the copy checks its typed cancellation before
open validation, so a close that cancelled it while waiting reports Cancelled
rather than an incidental closed-session failure.

## Added source tests — UNRUN

- Channel-gated genuine-failure/cancel/finalization race, repeated for evaluator,
  validation and write failure labels: exact failure is preserved, admission
  rejects a retry while gated, then admits fresh work after unwind.
- Typed checkpoint and EngineError::Cancelled map to Cancelled. A BridgeError
  even containing the text 'cancelled', an unrelated EngineError, and an IO
  failure remain errors. This specifically excludes string/flag inference.

Existing IO cancellation tests now inject typed cancellation. These tests target
the production finalizer and error conversions; they do not simulate every
encoder/OS failure or prove external kernels preserve their error precedence.

Source formatting with installed rustfmt and git diff --check passed.
No compilation, test, bindings generation, app, benchmark or heartbeat restart
on B. Generated API unchanged by this correction, but the original operation
still needs A's fresh bindings generation. A owns compiler, compositor follow-up,
frame telemetry and all main merges. No Swift/shell/render/compositor edits here.
