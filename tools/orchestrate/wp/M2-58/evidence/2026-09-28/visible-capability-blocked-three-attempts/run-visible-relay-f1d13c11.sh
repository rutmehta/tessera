#!/usr/bin/env bash
set -u
ROOT=/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera
APP=/Volumes/betterSSD/tessera-validation/m258-current/package-f1d13c11/TesseraM258Visible.app
BUNDLE_ID=dev.tessera.m258.visible.f1d13c11
COMMIT=f1d13c11ca1abe1c95b36176bb1e0c57d2b5b393
FIXTURE=/Users/rutmehta/Developer/tessera/fixtures/raw/sony-arw.ARW
OUT=/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11-tmpstdio
RELAY=/tmp/tessera-m258-visible-stdio-f1d13c11
RUNLOG=/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11-tmpstdio.log
if [[ -e "$OUT" || -e "$RELAY" ]]; then
  echo "fresh output or relay path already exists" >&2
  exit 2
fi
set +e
PYTHONDONTWRITEBYTECODE=1 python3 "$ROOT/tools/bench/app_timing_visible.py" \
  --app "$APP" --expected-bundle-id "$BUNDLE_ID" --expected-commit "$COMMIT" \
  --fixture "$FIXTURE" --output "$OUT" --stdio-relay-directory "$RELAY" > "$RUNLOG" 2>&1
rc=$?
printf '%s\n' "$rc" > "$OUT.exit"
set -e
python3 - "$OUT" "$RELAY" "$BUNDLE_ID" <<'PY'
import hashlib, json, pathlib, shutil, subprocess, sys, time
out = pathlib.Path(sys.argv[1])
relay = pathlib.Path(sys.argv[2])
bundle = sys.argv[3]
owner_path = out / "owned-app.json"
owner = json.loads(owner_path.read_text()) if owner_path.is_file() else None
settlement = {"owned_app_observed": owner is not None, "polls": 0, "process_exited": owner is None}
if owner:
    probe = out / "visible-window-probe"
    expected = (owner["pid"], owner["bundle_id"], owner["bundle_url"], owner["launch_date"])
    for index in range(80):
        settlement["polls"] = index + 1
        try:
            result = subprocess.run([str(probe), bundle], check=True, capture_output=True,
                                    text=True, timeout=5)
            apps = json.loads(result.stdout).get("bundle_apps", [])
            matching = [app for app in apps if (int(app["pid"]), app.get("bundle_id"),
                        app.get("bundle_url"), app.get("launch_date")) == expected]
            if not matching:
                settlement["process_exited"] = True
                break
        except Exception as error:
            settlement["probe_error"] = f"{type(error).__name__}: {error}"
            break
        time.sleep(.25)
    else:
        settlement["process_exited"] = False
settlement["owned_app"] = owner
settlement["wait_limit_seconds"] = 20
(out / "stdio-settlement.json").write_text(json.dumps(settlement, indent=2) + "\n")
for name in ("app-stdout.log", "app-stderr.log"):
    source = relay / name
    destination = out / name
    if source.is_file():
        shutil.copy2(source, destination)
manifest = {}
for name in ("app-stdout.log", "app-stderr.log"):
    source = relay / name
    destination = out / name
    for label, path in (("relay", source), ("copy", destination)):
        if path.is_file():
            manifest[f"{name}_{label}"] = hashlib.sha256(path.read_bytes()).hexdigest()
manifest["settlement"] = settlement
(out / "stdio-copies-sha256.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
print(json.dumps(manifest, indent=2, sort_keys=True))
PY
printf 'runner_direct_exit=%s\n' "$rc"
tail -50 "$RUNLOG"
exit 0
