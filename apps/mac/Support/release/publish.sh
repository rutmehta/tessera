#!/usr/bin/env bash
# Cut a release from the current commit: tag, Developer ID build, notarize, DMG, Sparkle
# appcast, GitHub release. Installed copies then update themselves through Sparkle.
#
#   Support/release/publish.sh 0.2.0            # tags v0.2.0 on HEAD and publishes it
#   Support/release/publish.sh 0.2.0 --notes f  # release notes from a file (default: the tag's commits)
#
# Needs, once per machine (see README.md): the Developer ID certificate in the login Keychain,
# a notarytool Keychain profile (NOTARY_PROFILE, default tessera-release), the Sparkle private
# key in the Keychain (generate_keys) with its public key in Support/Info.plist, and `gh` logged in.
set -euo pipefail
cd "$(dirname "$0")/../.."

version="${1:?usage: publish.sh MAJOR.MINOR.PATCH [--notes FILE]}"
notes_file=""
if [[ "${2:-}" == "--notes" ]]; then notes_file="${3:?--notes needs a file}"; fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1; }
tag="v$version"

export CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
export NOTARY_PROFILE="${NOTARY_PROFILE:-tessera-release}"
[[ -n "$CODESIGN_IDENTITY" ]] || { echo "no Developer ID Application identity in the Keychain" >&2; exit 1; }
if [[ "${ALLOW_UNNOTARIZED:-}" == "1" ]]; then
  # A stopgap release: signed with the Developer ID, not notarized (macOS asks the user to allow
  # it once on a fresh download; Sparkle updates are unaffected). Notarize and republish after.
  unset NOTARY_PROFILE
else
  xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1 \
    || { echo "notarytool profile '$NOTARY_PROFILE' missing: run xcrun notarytool store-credentials $NOTARY_PROFILE (or ALLOW_UNNOTARIZED=1)" >&2; exit 1; }
fi
[[ -n "$(/usr/libexec/PlistBuddy -c 'Print :SUPublicEDKey' Support/Info.plist 2>/dev/null)" ]] \
  || { echo "SUPublicEDKey is empty in Support/Info.plist; run generate_keys and paste the public key" >&2; exit 1; }
[[ -z "$(git status --porcelain --untracked-files=no)" ]] || { echo "working tree is not clean; commit first" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && { echo "$tag already exists" >&2; exit 1; }

previous="$(git describe --tags --match 'v[0-9]*' --abbrev=0 2>/dev/null || true)"
git tag -a "$tag" -m "Tessera $version"

echo "== build $tag with $CODESIGN_IDENTITY"
bash Support/make-app.sh release
echo "== notarize the app"
bash Support/release/notarize.sh
echo "== disk image"
rm -rf dist && mkdir -p dist
# Earlier DMGs let generate_appcast emit deltas (up to five per latest version).
if [[ -n "$previous" ]]; then
  gh release download "$previous" --pattern '*.dmg' --dir dist --skip-existing 2>/dev/null || true
fi
bash Support/release/make-dmg.sh
dmg="dist/Tessera-$version.dmg"
[[ -f "$dmg" ]] || { echo "expected $dmg" >&2; exit 1; }
echo "== notarize the disk image"
bash Support/release/notarize.sh "$dmg"
echo "== appcast"
export SPARKLE_DOWNLOAD_URL_PREFIX="https://github.com/rutmehta/tessera/releases/download/$tag/"
bash Support/release/make-appcast.sh dist

echo "== publish $tag"
git push origin HEAD "$tag"
if [[ -z "$notes_file" ]]; then
  notes_file="$(mktemp)"
  { echo "## Tessera $version"; echo
    if [[ -n "$previous" ]]; then git log --no-merges --format='- %s' "$previous..HEAD"; else git log --no-merges --format='- %s' -20; fi
  } > "$notes_file"
fi
assets=("$dmg" dist/appcast.xml)
for delta in dist/*.delta; do [[ -f "$delta" ]] && assets+=("$delta"); done
gh release create "$tag" "${assets[@]}" --title "Tessera $version" --notes-file "$notes_file"
echo "published https://github.com/rutmehta/tessera/releases/tag/$tag"
