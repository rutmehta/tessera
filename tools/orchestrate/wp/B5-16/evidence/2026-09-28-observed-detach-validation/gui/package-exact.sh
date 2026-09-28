#!/bin/bash
set -euo pipefail

ROOT=/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera
SCRATCH=/Volumes/betterSSD/tessera-cache/swift/document-save-settlement/scratch/arm64-apple-macosx/release
EVIDENCE=/Volumes/betterSSD/tessera-validation/document-save-settlement/observed-detach-2a94a187/gui
APP="$EVIDENCE/Tessera Observed Detach 2a94.app"

test "$(git -C "$ROOT" rev-parse HEAD)" = 2a94a1878a760b41c6e6aa2c80be7a5feb60eeee
test -z "$(git -C "$ROOT" status --porcelain=v1)"
test ! -e "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$SCRATCH/Tessera" "$APP/Contents/MacOS/Tessera"
cp "$ROOT/apps/mac/Support/Info.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier dev.tessera.document-save-observed.2a94a187' "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleName Tessera Observed Detach 2a94' "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleShortVersionString 0.0.1' "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleVersion 1' "$APP/Contents/Info.plist"
ditto "$SCRATCH/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
install_name_tool -add_rpath '@executable_path/../Frameworks' "$APP/Contents/MacOS/Tessera"
printf 'APPL????' > "$APP/Contents/PkgInfo"
FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
for helper in \
  "$FRAMEWORK/Versions/B/XPCServices/Downloader.xpc" \
  "$FRAMEWORK/Versions/B/XPCServices/Installer.xpc" \
  "$FRAMEWORK/Versions/B/Autoupdate" \
  "$FRAMEWORK/Versions/B/Updater.app"; do
  codesign --force --sign - --options runtime --preserve-metadata=entitlements "$helper"
done
codesign --force --sign - --options runtime "$FRAMEWORK"
codesign --force --sign - --options runtime \
  --entitlements "$ROOT/apps/mac/Support/release/Tessera-adhoc.entitlements" "$APP"
codesign --verify --deep --strict "$APP"
shasum -a 256 "$SCRATCH/Tessera" "$APP/Contents/MacOS/Tessera" \
  "$APP/Contents/Info.plist" "$ROOT/apps/mac/build/ffi/libtessera_ffi.a" \
  > "$EVIDENCE/package-hashes.txt"
