#!/bin/bash
set -euo pipefail

ROOT=/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera
SCRATCH=/Volumes/betterSSD/tessera-validation/document-save-settlement/candidate-62d546dc-focused-4a45-20260928/scratch/arm64-apple-macosx/release
EVIDENCE=/Volumes/betterSSD/tessera-validation/document-save-settlement/owned-presenter-d695e6a1/gui
APP="$EVIDENCE/Tessera Owned Save d695.app"

test "$(git -C "$ROOT" rev-parse HEAD)" = d695e6a12b5f6753bc1da6c63bd1568d2ef20ab7
test -z "$(git -C "$ROOT" status --porcelain=v1)"
test "$(shasum -a 256 "$ROOT/apps/mac/build/ffi/libtessera_ffi.a" | awk '{print $1}')" = 4a45f8235a8c6382d0dad9be735d5cd336a86c20eb50c15051b1c55654444596
test "$(shasum -a 256 "$SCRATCH/Tessera" | awk '{print $1}')" = 5038548a2753ec4c993146379ee9ba33cfcf0e470329e8287e867f4d076e1c9e
test ! -e "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$SCRATCH/Tessera" "$APP/Contents/MacOS/Tessera"
cp "$ROOT/apps/mac/Support/Info.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier dev.tessera.document-save-owned.d695e6a1' "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleName Tessera Owned Save d695' "$APP/Contents/Info.plist"
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
printf '%s\n' "$APP" > "$EVIDENCE/package-path.txt"
