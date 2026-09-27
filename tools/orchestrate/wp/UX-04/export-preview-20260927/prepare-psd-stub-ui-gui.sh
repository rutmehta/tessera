#!/bin/zsh
set -euo pipefail

repo_root=${1:?usage: prepare-psd-stub-ui-gui.sh <repo-root> [new-empty-temp-root]}
run_root=${2:-/tmp/tessera-psd-stub-ui-gui-20260927}
if [[ -e "$run_root" ]]; then
  print -u2 "Refusing to reuse existing validation directory: $run_root"
  exit 2
fi
fixture_root="$repo_root/tools/orchestrate/wp/UX-04/export-preview-20260927/fixtures/jpeg-output"
fixture_name='Fixture long name - display proof test.jpg'
mkdir -p "$run_root/catalog" "$run_root/psd-output" "$run_root/support-normal" "$run_root/support-diagnostic"
cp -p "$fixture_root/$fixture_name" "$run_root/catalog/$fixture_name"
cp -p "$fixture_root/$fixture_name.xmp" "$run_root/catalog/$fixture_name.xmp"
(
  cd "$run_root/catalog"
  shasum -a 256 "$fixture_name" "$fixture_name.xmp"
) > "$run_root/input-before.sha256"
print "Prepared disposable GUI input at $run_root"
print "No app was launched and no build was run."
