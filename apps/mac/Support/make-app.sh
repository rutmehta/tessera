#!/usr/bin/env bash
# Builds the Tessera executable with SwiftPM and wraps it into apps/mac/build/Tessera.app
# (bundle id dev.tessera.app), with Sparkle and inside-out code signing.
#
#   Support/make-app.sh            # optimized release build with provenance
#   Support/make-app.sh debug      # explicit debug package (not benchmark eligible)
set -euo pipefail
cd "$(dirname "$0")/.."
CONFIG="${1:-release}"
case "$CONFIG" in debug|release) ;; *) echo "Usage: $0 [debug|release]" >&2; exit 2;; esac
ROOT="$(cd ../.. && pwd)"
# Preserve caller isolation; build-ffi.sh uses the same default when unset.
# One target dir per checkout (name + short hash of the full path, since many
# worktrees share the basename "tessera"): worktrees building concurrently must not overwrite each
# other's libtessera_ffi and generate bindings from the wrong source.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/tessera-target/mac-ffi-$(basename "$ROOT")-$(printf %s "$ROOT" | shasum | cut -c1-8)}"
mkdir -p build
# A new Swift scratch tree guarantees a fresh link of the just-built archive.
# Keep it for provenance verification; everyday .build debug tooling is untouched.
SCRATCH="$(mktemp -d "$(pwd)/build/provenance-swift.XXXXXX")"
SNAPSHOT="$SCRATCH/source-snapshot.json"
python3 Support/provenance.py snapshot --root "$ROOT" --snapshot "$SNAPSHOT"
bash ./build-ffi.sh
python3 Support/provenance.py ffi --root "$ROOT" --target "$CARGO_TARGET_DIR" --snapshot "$SNAPSHOT"
swift build --scratch-path "$SCRATCH" -c "$CONFIG" --product Tessera
BIN_DIR="$(swift build --scratch-path "$SCRATCH" -c "$CONFIG" --show-bin-path)"
APP="build/Tessera.app"
rm -f "$APP.provenance.json"
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
# Hash the packaged executable after install_name_tool, before bundle signing.
python3 Support/provenance.py record --root "$ROOT" --app "$APP" \
  --bin-dir "$BIN_DIR" --configuration "$CONFIG" --target "$CARGO_TARGET_DIR" --snapshot "$SNAPSHOT"
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
# codesign mutates the Mach-O. Seal its final bytes OUTSIDE the bundle to avoid
# changing signed resources and creating a circular signing/hash dependency.
python3 Support/provenance.py seal --app "$APP"
if [[ "$CONFIG" == debug ]]; then
  python3 Support/provenance.py verify --root "$ROOT" --app "$APP" --allow-debug
else
  python3 Support/provenance.py verify --root "$ROOT" --app "$APP"
fi
echo "Built $(pwd)/$APP"
