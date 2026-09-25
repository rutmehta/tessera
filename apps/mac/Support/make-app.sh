#!/usr/bin/env bash
# Builds the Tessera executable with SwiftPM and wraps it into apps/mac/build/Tessera.app
# (bundle id dev.tessera.app), with Sparkle and inside-out code signing.
#
#   Support/make-app.sh            # debug build
#   Support/make-app.sh release    # optimised build (use this for the 20k-item scroll benchmark)
set -euo pipefail
cd "$(dirname "$0")/.."
CONFIG="${1:-debug}"
case "$CONFIG" in debug|release) ;; *) echo "Usage: $0 [debug|release]" >&2; exit 2;; esac
swift build -c "$CONFIG" --product Tessera
BIN_DIR="$(swift build -c "$CONFIG" --show-bin-path)"
APP="build/Tessera.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$BIN_DIR/Tessera" "$APP/Contents/MacOS/Tessera"
cp Support/Info.plist "$APP/Contents/Info.plist"
VERSION="$(bash Support/release/version.sh)"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP/Contents/Info.plist"
if [[ -z "$(/usr/libexec/PlistBuddy -c 'Print :SUPublicEDKey' "$APP/Contents/Info.plist")" ]]; then
  echo "warning: SUPublicEDKey is empty; signed Sparkle updates require release key setup." >&2
fi
# ditto follows the outer SPM symlink but preserves the framework's internal symlinks.
ditto "$BIN_DIR/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
install_name_tool -add_rpath '@executable_path/../Frameworks' "$APP/Contents/MacOS/Tessera"
printf 'APPL????' > "$APP/Contents/PkgInfo"
IDENTITY="${CODESIGN_IDENTITY:--}"
SIGN=(--force --sign "$IDENTITY" --options runtime)
ENTITLEMENTS=Support/release/Tessera.entitlements
if [[ "$IDENTITY" == - ]]; then
  # Ad-hoc binaries have no shared Team ID. Hardened library validation would
  # reject the separately signed Sparkle framework before main() even runs.
  ENTITLEMENTS=Support/release/Tessera-adhoc.entitlements
else
  SIGN+=(--timestamp)
fi
FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
for helper in "$FRAMEWORK/Versions/B/XPCServices/Downloader.xpc" \
              "$FRAMEWORK/Versions/B/XPCServices/Installer.xpc" \
              "$FRAMEWORK/Versions/B/Autoupdate" "$FRAMEWORK/Versions/B/Updater.app"; do
  # Keep Sparkle's sandbox entitlements on its downloader helper.
  codesign "${SIGN[@]}" --preserve-metadata=entitlements "$helper"
done
codesign "${SIGN[@]}" "$FRAMEWORK"
codesign "${SIGN[@]}" --entitlements "$ENTITLEMENTS" "$APP"
codesign --verify --deep --strict "$APP"
echo "Built $(pwd)/$APP"
