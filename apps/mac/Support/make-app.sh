#!/usr/bin/env bash
# Builds the PhotoEditor executable with SwiftPM and wraps it into apps/mac/build/PhotoEditor.app
# (bundle id dev.local.photoeditor), ad-hoc signed so it launches like a normal app.
#
#   Support/make-app.sh            # debug build
#   Support/make-app.sh release    # optimised build (use this for the 20k-item scroll benchmark)
set -euo pipefail
cd "$(dirname "$0")/.."
CONFIG="${1:-debug}"
swift build -c "$CONFIG" --product PhotoEditor
BIN="$(swift build -c "$CONFIG" --show-bin-path)/PhotoEditor"
APP="build/PhotoEditor.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/PhotoEditor"
cp Support/Info.plist "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "warning: ad-hoc codesign failed (app still runs locally)"
echo "Built $(pwd)/$APP"
