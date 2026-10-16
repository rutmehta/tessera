#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
ARTIFACT="${1:-build/Tessera.app}"
[[ -e "$ARTIFACT" ]] || { echo "error: missing artifact: $ARTIFACT" >&2; exit 1; }
# Credentials: an App Store Connect API key file (NOTARY_KEY_PATH + NOTARY_KEY_ID + NOTARY_ISSUER)
# is preferred; it never touches the login Keychain, which has silently lost the notarytool
# profile on this Mac more than once. Otherwise a notarytool Keychain profile (NOTARY_PROFILE).
NOTARY_AUTH=()
if [[ -n "${NOTARY_KEY_PATH:-}" && -n "${NOTARY_KEY_ID:-}" && -n "${NOTARY_ISSUER:-}" ]]; then
  NOTARY_AUTH=(--key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
elif [[ -n "${NOTARY_PROFILE:-}" ]]; then
  NOTARY_AUTH=(--keychain-profile "$NOTARY_PROFILE")
fi
if [[ ${#NOTARY_AUTH[@]} -eq 0 ]]; then
  echo "Skipping notarization: no NOTARY_KEY_PATH/NOTARY_KEY_ID/NOTARY_ISSUER and no NOTARY_PROFILE (not a notarized release)."
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
  if xcrun notarytool submit "$UPLOAD" "${NOTARY_AUTH[@]}" --wait; then submitted=1; break; fi
  echo "notarytool submit failed (attempt $attempt); retrying" >&2
  sleep 15
done
[[ "$submitted" == 1 ]] || { echo "notarization failed after 3 attempts" >&2; exit 1; }
xcrun stapler staple "$ARTIFACT"
xcrun stapler validate "$ARTIFACT"
