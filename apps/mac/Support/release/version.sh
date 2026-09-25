#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
# Numeric release tags sort correctly in Sparkle. Untagged development builds
# use a clearly non-release fallback rather than a hexadecimal bundle version.
description="$(git describe --tags --match 'v[0-9]*' --always)"
if [[ "$description" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  printf '%s\n' "${BASH_REMATCH[1]}"
else
  printf '0.0.0-dev.%s\n' "$description"
fi
