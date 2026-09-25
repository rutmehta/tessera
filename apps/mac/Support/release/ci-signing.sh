#!/usr/bin/env bash
# Called only on an ephemeral GitHub runner. Never log certificate/key material.
set -euo pipefail
set +x
: "${RUNNER_TEMP:?CI only}" "${GITHUB_ENV:?CI only}"
if [[ -z "${APPLE_CERT_P12:-}" || -z "${APPLE_CERT_PASSWORD:-}" ||
      -z "${NOTARY_APPLE_ID:-}" || -z "${NOTARY_PASSWORD:-}" || -z "${NOTARY_TEAM_ID:-}" ]]; then
  echo 'Apple signing/notary secrets incomplete: using ad-hoc signing, no notarization.'
  printf 'CODESIGN_IDENTITY=-\n' >> "$GITHUB_ENV"
  exit 0
fi
KEYCHAIN="$RUNNER_TEMP/tessera-release.keychain-db"
PASSWORD="$(openssl rand -hex 24)"
printf '::add-mask::%s\n' "$PASSWORD"
python3 - <<'PY'
import base64, os
from pathlib import Path
p = Path(os.environ['RUNNER_TEMP']) / 'tessera-certificate.p12'
p.write_bytes(base64.b64decode(os.environ['APPLE_CERT_P12'], validate=True))
p.chmod(0o600)
PY
security create-keychain -p "$PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$PASSWORD" "$KEYCHAIN"
security import "$RUNNER_TEMP/tessera-certificate.p12" -P "$APPLE_CERT_PASSWORD" -k "$KEYCHAIN" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$PASSWORD" "$KEYCHAIN" >/dev/null
security list-keychains -d user -s "$KEYCHAIN" "$HOME/Library/Keychains/login.keychain-db"
IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | python3 -c 'import re,sys; s=sys.stdin.read(); m=re.search(r"([0-9A-F]{40}) .*Developer ID Application:", s); sys.exit("No Developer ID Application identity") if not m else print(m[1])')"
printf 'CODESIGN_IDENTITY=%s\n' "$IDENTITY" >> "$GITHUB_ENV"
xcrun notarytool store-credentials tessera-release --apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD"
printf 'NOTARY_PROFILE=tessera-release\n' >> "$GITHUB_ENV"
rm -f "$RUNNER_TEMP/tessera-certificate.p12"
