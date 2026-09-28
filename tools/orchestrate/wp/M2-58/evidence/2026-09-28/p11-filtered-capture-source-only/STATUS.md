# Source-only filtered capture capability — UNCOMPILED / UNRUN

Prepared on 2026-09-28 and independently reviewed by the Machine A coordinator before preservation. No build, ScreenCaptureKit stream, screen capture, permission request or product change is claimed by this checkpoint. Runtime remains under the coordinator native lane.

The standalone helper checks its own existing screen-capture access, requires the exact isolated BetterSSD test app PID/bundle/path/launch date/window/display/ROI, and restricts pixels to a display filter containing only that test window. It saves bounded complete-frame BGRA buffers and raw WindowServer display timestamps/status/geometry, with bounded startup/stop and a nine-second watchdog.

This is filtered API/pixels/timestamps capability only. It cannot establish unobscured visible detail or satisfy P11 latency. No exact-detail matcher, input/revision evaluator, benchmark interval or acceptance oracle is implemented. It does not request authorization or modify system settings. The README preserves preparation paths for provenance; the adjacent source is the durable copy.

Source SHA256: `2c6f62df83df2082d0a3b8f4bc3c746ac7b8b90fc17d2fd5aa1051c57cbf8e2e`.

Coordinator-requested source corrections are included: fail before capture if stream configuration metadata cannot be written; unlock and fail on a NULL pixel buffer base address; record concrete pre-capture identity/window/ROI/add-output failures.
