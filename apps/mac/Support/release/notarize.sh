#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
ARTIFACT="${1:-build/Tessera.app}"
[[ -e "$ARTIFACT" ]] || { echo "error: missing artifact: $ARTIFACT" >&2; exit 1; }
if [[ -z "${NOTARY_PROFILE:-}" ]]; then
  echo "Skipping notarization: NOTARY_PROFILE is not set (not a notarized release)."
  exit 0
fi
UPLOAD="$ARTIFACT"
if [[ "$ARTIFACT" == *.app ]]; then
  UPLOAD="build/notary-submit.zip"
  trap 'rm -f "$UPLOAD"' EXIT
  ditto -c -k --sequesterRsrc --keepParent "$ARTIFACT" "$UPLOAD"
fi
xcrun notarytool submit "$UPLOAD" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$ARTIFACT"
xcrun stapler validate "$ARTIFACT"
