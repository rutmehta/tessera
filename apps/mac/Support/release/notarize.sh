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
# notarytool crashes (SIGBUS, a stack overflow in its own progress formatting) while it waits
# with `--wait` on this Mac, often after the upload already succeeded. So: submit without
# waiting, take the submission id, and poll `notarytool info` until Apple answers.
submission=""
for attempt in 1 2 3; do
  out="$(xcrun notarytool submit "$UPLOAD" "${NOTARY_AUTH[@]}" --no-wait 2>&1 || true)"
  echo "$out"
  submission="$(printf '%s\n' "$out" | sed -n 's/^ *id: \([0-9a-f-]\{36\}\).*/\1/p' | head -1)"
  [[ -n "$submission" ]] && break
  echo "notarytool submit returned no submission id (attempt $attempt); retrying" >&2
  sleep 15
done
[[ -n "$submission" ]] || { echo "notarization: no submission id after 3 attempts" >&2; exit 1; }
status=""
for _ in $(seq 1 80); do
  sleep 15
  status="$(xcrun notarytool info "$submission" "${NOTARY_AUTH[@]}" 2>/dev/null | sed -n 's/^ *status: *//p' | head -1)"
  echo "notarization $submission: ${status:-no answer yet}"
  case "$status" in Accepted|Invalid|Rejected) break;; esac
done
if [[ "$status" != "Accepted" ]]; then
  xcrun notarytool log "$submission" "${NOTARY_AUTH[@]}" 2>&1 | head -60 >&2 || true
  echo "notarization failed: status '${status:-unknown}'" >&2
  exit 1
fi
xcrun stapler staple "$ARTIFACT"
xcrun stapler validate "$ARTIFACT"
