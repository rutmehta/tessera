# M2-45b workflow checkpoint

This checkpoint adds item 5 at the UniFFI host boundary plus CLI post-actions.
Item 2 (output sharpening) remains implemented. The overall work package is
still FAIL: metadata (1), DNG enhancements (3), and HDR (4) remain outstanding.

## Host APIs

- `Engine::export_batch` still accepts one `ExportOptions` JSON document.
- `Engine::export_multiple` accepts 1–32 such documents, including settings
  returned by `export_presets`. It resolves the selection once and executes
  presets serially, preserving selection order and each preset's destination,
  format, naming, and conflict policy. Progress restarts for each preset.
- `Engine::export_with_previous` applies the last successful settings list to
  a new selection. `<app-dir>/LastExport.json` is versioned and atomically
  replaced via a synced temporary file. No source IDs or recipes are saved.
  Reopening the engine retains it. Corrupt/unsupported documents fail rather
  than silently substituting defaults. Failed, partially failed, cancelled,
  or post-action-failed exports do not replace it.
- All settings JSON and absolute destination paths are preflighted before any
  preset starts. Runtime destination/name failures become per-preset reports,
  preserving earlier output paths. Multi-export is not a transaction: files
  already published remain, and cancellation omits later unstarted presets.
- `ExportReport.workflow_errors` separately reports host-action and persistence
  errors, without losing already exported paths or misclassifying images as
  failed renders. Consumers must inspect this field as well as `failed`.

## Actions

`ExportOptions.after_export` defaults to no actions and has these fields:

    {"reveal": false, "open_in_app": null, "run_script": null,
     "timeout_seconds": 60}

- Reveal and open-in-app use macOS `/usr/bin/open`, in that order, followed by
  the script. Program/app paths must be absolute. Script files must be
  executable, with their own shebang. Each output path is a distinct argv
  entry; the implementation never interpolates a shell command.
- The FFI host and CLI invoke `run_after_export` only after a completely
  successful, uncancelled image batch, after all encoding threads have joined.
  Each preset in a multi-export has its own actions. Source metadata and
  render/encode engine calls cannot invoke actions. Saving/normalizing a
  preset cannot invoke them either.
- Child stdin/stdout/stderr are disconnected so scripts cannot corrupt CLI
  JSON or wait on input. Exit failures are reported. Timeout is 1–3600 seconds
  per child; cancellation/timeout kills and reaps the direct child only,
  not processes it spawned. Opening a GUI app does not wait for the app to exit.
- The legacy `open_in_finder` field remains a UI hint, not an alias: the
  existing app already handles it, and executing it again would double-open.
- Actions are explicit opt-in settings and persist in presets/Previous. Do not
  load untrusted preset scripts as if they were harmless image metadata.

CLI flags: `--reveal`, `--open-in-app /absolute/App.app`,
`--after-export-script /absolute/script`, `--after-export-timeout 60`.
CLI JSON includes `workflow_errors` separately from image failures.

No UI controls or menu commands are added. Previous/multi-preset orchestration
is exposed through UniFFI; CLI and MCP Previous/multi-preset entry points are
not added in this checkpoint. Bindings must be regenerated together with the
new ExportReport layout before running the Swift host.

## Tests and evidence

- Failing tests first: `workflow-previous-red.log` (missing Previous API),
  `workflow-multi-red.log` (missing multi-preset API),
  `workflow-actions-red.log` (unknown action settings field), and
  `workflow-cli-red.log` (unknown script flag).
- Real FFI exports test reopening, a new selection, two formats in one call,
  remembering the complete list, invalid later settings before publication,
  runtime failure preserving earlier reports, cancellation, conflict failures,
  corrupt Previous documents, and preserving last-good settings.
- Executable test scripts check literal paths containing spaces/semicolons,
  publication before execution, nonzero exit reporting, and no execution on
  cancel or partial failure. CLI script stdout cannot corrupt JSON.
- Shared host-runner tests check direct-child timeout/reaping, pre-cancel,
  empty output lists, spawn failure, path/timeout validation, and exact macOS
  reveal/open argv. GUI commands are inspected, not actually opened by tests.
- Read-only independent review found no blocking defects. It did not execute
  tests or GUI commands. No codec or license dependency changed.

The full requested gate passed: 340 tests passed, 0 failed, 15 ignored, plus
Clippy, formatting, licenses, regenerated FFI and Swift build. Evidence is in
`workflow-verification.txt`. `workflow-gate.log` records a Clippy enum-size
failure fixed by boxing CLI Export options; `workflow-final-gate.log` records
a formatting-order failure fixed without changing behavior. A foreground
attempt in `workflow-verified-gate.log` was interrupted by the tool's 420-second
limit, not a completed gate. The final background invocation is
`workflow-complete-gate.log`. Logs are local and ignored.

## Remaining work

1. Expanded metadata policies and person/location removal, hierarchical
   keywords, EXIF/IPTC/XMP coverage and read-back in every metadata carrier.
3. Original + XMP copy, embedded original RAW toggle, DNG 1.6 audit, independent
   LibRaw RGB comparison of the developed float DNG.
4. PNG16/AVIF10/12 PQ/HLG with Rec.2020 tagging and interoperable ISO 21496-1
   gain-map JPEG with HDR reconstruction tests.

Lossy JPEG XL stays out of scope. No GPL codec was introduced.

## Integration hotspot

`crates/export/src/lib.rs` overlaps other export work: this checkpoint adds
only the workflow module and public re-exports. Preserve sibling changes.
Regenerate Swift/C bindings from the merged Rust API instead of mechanically
choosing one worker's generated binding files.
