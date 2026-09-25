#!/usr/bin/env bash
# Never generate a private key here. Missing signing credentials are fatal.
set -euo pipefail
set +x
cd "$(dirname "$0")/../.."
DIST="${1:-dist}"
BIN="${SPARKLE_BIN_DIR:-$PWD/.build/artifacts/sparkle/Sparkle/bin}"
ACCOUNT="${SPARKLE_KEY_ACCOUNT:-ed25519}"
[[ -d "$DIST" ]] || { echo "error: missing dist folder: $DIST" >&2; exit 1; }
[[ -x "$BIN/generate_appcast" ]] || { echo "error: run swift package resolve to install Sparkle tools" >&2; exit 1; }
ARGS=(--download-url-prefix "${SPARKLE_DOWNLOAD_URL_PREFIX:-https://github.com/rutmehta/tessera/releases/latest/download/}" --maximum-deltas 5 -o "$DIST/appcast.xml")
if [[ -n "${SPARKLE_PRIVATE_KEY:-}" ]]; then
  printf '%s' "$SPARKLE_PRIVATE_KEY" | "$BIN/generate_appcast" --ed-key-file - "${ARGS[@]}" "$DIST"
else
  # Query only existence, never read or print a Keychain secret.
  if ! security find-generic-password -s https://sparkle-project.org -a "$ACCOUNT" >/dev/null 2>&1; then
    echo "error: no Sparkle signing key; run generate_keys once or set SPARKLE_PRIVATE_KEY (see Support/release/README.md)." >&2
    exit 1
  fi
  "$BIN/generate_appcast" --account "$ACCOUNT" "${ARGS[@]}" "$DIST"
fi
[[ -s "$DIST/appcast.xml" ]] || { echo "error: Sparkle did not produce appcast.xml" >&2; exit 1; }
# Fail closed even if upstream merely warns about unsigned/unusable archives.
python3 - "$DIST/appcast.xml" <<'PY'
import sys
import xml.etree.ElementTree as ET
items = ET.parse(sys.argv[1]).findall('.//enclosure')
key = '{http://www.andymatuschak.org/xml-namespaces/sparkle}edSignature'
if not items or any(not item.get(key) for item in items):
    sys.exit('error: appcast has no updates or contains an unsigned update')
PY
