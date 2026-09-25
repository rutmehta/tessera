#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
CODESIGN_IDENTITY=- bash Support/make-app.sh release
APP=build/Tessera.app
codesign --verify --deep --strict "$APP"
FW="$APP/Contents/Frameworks/Sparkle.framework"
test -f "$FW/Sparkle"
test -L "$FW/Versions/Current"
test -d "$FW/Versions/B/XPCServices/Installer.xpc"
test -d "$FW/Versions/B/XPCServices/Downloader.xpc"
python3 - "$APP" <<'PY'
import plistlib, subprocess, sys
from pathlib import Path
app = Path(sys.argv[1])
p = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
assert p['SUEnableAutomaticChecks'] and p['SUScheduledCheckInterval'] == 86400
assert p['SUFeedURL'] == 'https://github.com/rutmehta/tessera/releases/latest/download/appcast.xml'
assert 'SUPublicEDKey' in p
info = subprocess.check_output(['codesign', '-dv', str(app)], stderr=subprocess.STDOUT).decode()
assert 'runtime' in info and 'Signature=adhoc' in info
PY
# A unique nonexistent account isolates this test from the developer's real key.
TEST="$(mktemp -d "$PWD/build/appcast-negative.XXXXXX")"
trap 'rm -rf "$TEST"' EXIT
python3 - "$TEST" <<'PY'
from pathlib import Path
import sys, zipfile
with zipfile.ZipFile(Path(sys.argv[1]) / 'dummy.zip', 'w') as z:
    z.writestr('dummy.txt', 'Not an application and deliberately unsigned')
PY
if SPARKLE_PRIVATE_KEY= SPARKLE_KEY_ACCOUNT="tessera-test-missing-$(uuidgen)"    bash Support/release/make-appcast.sh "$TEST" > "$TEST/output.log" 2>&1; then
  echo 'FAIL: unsigned dummy unexpectedly accepted' >&2; exit 1
fi
python3 - "$TEST/output.log" <<'PY'
from pathlib import Path
import sys
message = Path(sys.argv[1]).read_text()
assert 'error: no Sparkle signing key;' in message, message
print(message, end='')
PY
test ! -e "$TEST/appcast.xml"
echo 'PASS: ad-hoc bundle, embedded Sparkle/XPCs, hardened runtime, and missing-key rejection'
