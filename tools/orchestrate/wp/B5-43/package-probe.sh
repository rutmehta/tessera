#!/bin/bash
# Package the exact release executable from the hosted-test build. No rebuild or install.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
bin="$PWD/apps/mac/.build/arm64-apple-macosx/release"
app="$PWD/apps/mac/build/Tessera.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
cp "$bin/Tessera" "$app/Contents/MacOS/Tessera"
cp apps/mac/Support/Info.plist "$app/Contents/Info.plist"
ditto "$bin/Sparkle.framework" "$app/Contents/Frameworks/Sparkle.framework"
install_name_tool -add_rpath '@executable_path/../Frameworks' "$app/Contents/MacOS/Tessera"
framework="$app/Contents/Frameworks/Sparkle.framework"
for helper in "$framework/Versions/B/XPCServices/Downloader.xpc" \
              "$framework/Versions/B/XPCServices/Installer.xpc" \
              "$framework/Versions/B/Autoupdate" "$framework/Versions/B/Updater.app"; do
  codesign --force --sign - --options runtime --preserve-metadata=entitlements "$helper"
done
codesign --force --sign - --options runtime "$framework"
codesign --force --sign - --options runtime --entitlements apps/mac/Support/release/Tessera-adhoc.entitlements "$app"
codesign --verify --deep --strict "$app"
shasum -a 256 "$bin/Tessera" "$app/Contents/MacOS/Tessera"
