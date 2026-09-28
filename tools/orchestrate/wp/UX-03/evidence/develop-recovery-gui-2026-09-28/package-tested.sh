#!/bin/bash
set -euo pipefail

ROOT=/Users/rutmehta/.codex/worktrees/review-ownership/tessera
: "${TESTED_HEAD:?Set the exact full-suite tested Git HEAD}"
: "${TESTED_RELEASE_DIR:?Set the exact tested arm64 release directory}"
: "${EXPECTED_EXE_SHA256:?Set the recorded SHA-256 of the tested executable}"
: "${EVIDENCE_DIR:?Set a fresh unique betterSSD evidence directory}"
SCRATCH="$TESTED_RELEASE_DIR"
EVIDENCE="$EVIDENCE_DIR"
SHORT="${TESTED_HEAD:0:8}"
APP="$EVIDENCE/Tessera Develop Recovery $SHORT.app"

test "$(git -C "$ROOT" rev-parse HEAD)" = "$TESTED_HEAD"
test -z "$(git -C "$ROOT" status --porcelain=v1)"
test ! -e "$APP"
test -f "$SCRATCH/Tessera"
test "$(shasum -a 256 "$SCRATCH/Tessera" | awk '{print $1}')" = "$EXPECTED_EXE_SHA256"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$SCRATCH/Tessera" "$APP/Contents/MacOS/Tessera"
cp "$ROOT/apps/mac/Support/Info.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier dev.tessera.validation.developrecovery.$SHORT" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName Tessera Develop Recovery $SHORT" "$APP/Contents/Info.plist"
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
