#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
APP=build/Tessera.app
codesign --verify --deep --strict "$APP"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
mkdir -p dist
STAGE="$(mktemp -d "$PWD/build/dmg-stage.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/Tessera.app"
ln -s /Applications "$STAGE/Applications"
DMG="dist/Tessera-$VERSION.dmg"
hdiutil create -ov -volname Tessera -srcfolder "$STAGE" -fs APFS -format UDZO "$DMG"
if [[ "${CODESIGN_IDENTITY:--}" != - ]]; then
  codesign --force --sign "$CODESIGN_IDENTITY" --timestamp "$DMG"
fi
hdiutil verify "$DMG"
echo "Built $PWD/$DMG"
