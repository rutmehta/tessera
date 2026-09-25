#!/usr/bin/env bash
# Builds the Tessera executable with SwiftPM and wraps it into apps/mac/build/Tessera.app
# (bundle id dev.tessera.app), ad-hoc signed so it launches like a normal app.
#
#   Support/make-app.sh            # debug build
#   Support/make-app.sh release    # optimised build (use this for the 20k-item scroll benchmark)
set -euo pipefail
cd "$(dirname "$0")/.."
CONFIG="${1:-debug}"
swift build -c "$CONFIG" --product Tessera
BIN="$(swift build -c "$CONFIG" --show-bin-path)/Tessera"
APP="build/Tessera.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Tessera"
cp Support/Info.plist "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "warning: ad-hoc codesign failed (app still runs locally)"
echo "Built $(pwd)/$APP"
