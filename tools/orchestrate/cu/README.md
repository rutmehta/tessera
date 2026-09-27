# Background computer use from the Codex CLI (Machine B, 2026-09-27)

The ChatGPT desktop app installs Codex's background computer-use service (`~/.codex/computer-use/Codex Computer Use.app`, `com.openai.sky.CUAService`), and the Codex CLI reaches it through the bundled `cua_repl` MCP server (`sky.list_apps`, `get_app_state` for a screenshot plus accessibility tree, `click`, `type_text`, `press_key`, `scroll`, `drag`, `set_value`). Apps are driven with an invisible cursor and never brought to the front.

`codex exec` cannot use it for real work: it forces `approval: never`, so the per-app "Allow Computer Use to use <app>?" prompt is always declined (config `computer_use.macos.bundle_ids` allow-lists did not help as of CLI 0.157.0). `cu_client.py` runs `codex app-server` instead and accepts that prompt **only** for bundle ids in `CU_ALLOW`, from the `cua_repl` server; every other approval (shell, sandbox, other apps) is declined.

```sh
CU_ALLOW=dev.tessera.app CU_MODEL=gpt-6-sol CU_CWD=/Users/rutmehta/Developer/lightroom \
  python3 tools/orchestrate/cu/cu_client.py "Use cua_repl: sky.get_app_state({app:'dev.tessera.app'}) …"
```

- The thread runs with `sandbox: "read-only"`: build the app first (separately), then drive the prebuilt bundle.
- Verified read-only on Finder (screenshot + AX tree, Finder stayed in the background). Clicking/typing into Tessera through it is not yet verified.
- `codex app-server` is experimental; after upgrading the CLI, regenerate types with `codex app-server generate-ts --out DIR` and re-test.
- Permissions: Screen Recording and Accessibility for "Codex Computer Use" (already granted on this Mac).
- Fallbacks: Claude desktop background computer use (Settings ▸ Desktop app ▸ Computer use), or a small MCP using `CGEvent.postToPid` + `AXUIElementPerformAction` + `screencapture -l` (see OpenCodexLabs/open-codex-computer-use).
Sources: https://learn.chatgpt.com/docs/computer-use.md, https://learn.chatgpt.com/docs/config-file/config-reference, https://github.com/openai/codex/issues/19544, https://github.com/openai/codex/issues/20851
