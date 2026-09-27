# Stub-library UI gating source note — unbuilt

Source-only follow-up after validation exposed `Load 20,000 Stub Items` in both the empty Library view and the Debug command menu.

The two interactive UI entry points now appear only when the process is explicitly marked with `--enable-stub-library` or `TESSERA_ENABLE_STUB_LIBRARY=1`. This is an opt-in for the UI controls, not a change to diagnostic launch behavior: existing direct `--stub` / `--stub-count` launches and the `--stub-library` document-mode policy remain untouched. The normal editing-preview package does not set the new flag, so its ordinary empty Library omits the synthetic-content affordance.

This candidate is **unbuilt and untested**. Later verification should check ordinary launch hides both entry points and a diagnostic launch with the new flag exposes them. No PSD-slot build was run for this change.
