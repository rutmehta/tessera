#!/bin/bash
set -euo pipefail
ROOT=/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera
SCRATCH=/Volumes/betterSSD/tessera-cache/swift/document-save-settlement/scratch/arm64-apple-macosx/release
EVIDENCE=/Volumes/betterSSD/tessera-validation/document-save-settlement/terminal-trace-no-observable-e51ef7fd/gui
APP="$EVIDENCE/Tessera Terminal Trace e51ef7fd.app"
test "$(git -C "$ROOT" rev-parse HEAD)" = e51ef7fd9e1b6d82b69e32728d7986e481420ea7
test -z "$(git -C "$ROOT" status --porcelain=v1)"
test "$(shasum -a 256 "$SCRATCH/Tessera" | awk '{print $1}')" = 85f82718b4a59cfdf8a15276633be57c0eef7fe34996e888374690b50c5f7527
test ! -e "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$SCRATCH/Tessera" "$APP/Contents/MacOS/Tessera"
cp "$ROOT/apps/mac/Support/Info.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier dev.tessera.document-save-trace.e51ef7fd' "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleName Tessera Terminal Trace e51ef7fd' "$APP/Contents/Info.plist"
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
