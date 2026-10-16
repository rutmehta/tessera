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
# notarytool has crashed (bus error) mid-submission on this Mac; a second attempt goes through.
submitted=0
for attempt in 1 2 3; do
  if xcrun notarytool submit "$UPLOAD" --keychain-profile "$NOTARY_PROFILE" --wait; then submitted=1; break; fi
  echo "notarytool submit failed (attempt $attempt); retrying" >&2
  sleep 15
done
[[ "$submitted" == 1 ]] || { echo "notarization failed after 3 attempts" >&2; exit 1; }
xcrun stapler staple "$ARTIFACT"
xcrun stapler validate "$ARTIFACT"
