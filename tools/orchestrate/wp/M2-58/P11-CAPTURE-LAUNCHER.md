# Bounded launcher for the filtered capture capability helper

`p11_capture_launcher.py` accepts one explicit compiled helper, one configuration JSON, and a new receipt path. It invokes that helper directly (no shell), with a 15-second process deadline. The helper reads the immutable JSON snapshot supplied by the launcher, so configuration parsing and the helper's initial output-directory creation are inside the bounded child lifetime. On timeout the launcher signals only the `Popen` child it created, then waits at most two more seconds to reap it. It never looks up or terminates Tessera by name, PID, or bundle ID. A cleanup that misses that reap bound is recorded as incomplete and returns 125; timeout returns 124 and is not represented as a helper exit code.

Before launching, the wrapper validates the explicit isolated `.app` bundle, its `CFBundleIdentifier`, the `dev.tessera.m258.*` namespace, BetterSSD paths, a not-yet-existing output directory with an existing parent, and the positive PID/launch epoch/window/display plus finite bounded ROI fields. The helper itself remains responsible for checking that those exact runtime identities and current foreground/window geometry still match. The wrapper does not ask macOS for Screen Recording permission or launch/activate/terminate any app.

The receipt records the explicit PID, bundle ID, canonical app path, launch epoch, window/display IDs and ROI; launcher/helper/original-config/canonical-snapshot SHA-256 values; command, timeout, child PID, direct exit or timeout, and bounded stdout/stderr tails. Sibling log files and the canonical validated config snapshot passed to the helper are exclusive-create and retained. An existing receipt, snapshot, log, or helper output directory is a refusal condition. Integrity mismatches return 126 while preserving the helper's separate `direct_exit`; a timeout returns 124, and a child that cannot be reaped within the bounded cleanup interval returns 125.

The hard deadline bounds the helper process from spawn through config parsing and its own directory creation. Python-side initial file reads, hashing, record writes, process creation, and filesystem operations are not themselves interruptible; this is therefore a bounded helper invocation, not a universal hard wall-clock guarantee under a stalled filesystem. No capture, permission request, app launch, or desktop interaction is part of developing or testing this launcher. The compiled helper's existing source limitations still apply: the capture is a selected-window API capability and does not establish unobscured pixels, detail matching, or P11 latency acceptance.

Run only the harmless launcher stand-in tests with:

```sh
python3 -m unittest -v tools/orchestrate/wp/M2-58/tests/test_p11_capture_launcher.py
```

The tests use temporary fake app bundles and Python stand-ins, not the compiled capture helper or a running Tessera app.
