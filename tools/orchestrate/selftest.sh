#!/usr/bin/env bash
set -euo pipefail
SRC="$(git rev-parse --show-toplevel)"; TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/repo/tools/orchestrate"; cp "$SRC/tools/orchestrate/run-luna.sh" "$TMP/repo/tools/orchestrate/"; cd "$TMP/repo"
git init -q -b main; git config user.email selftest@example.invalid; git config user.name Selftest
mkdir -p tools/orchestrate/wp/DUMMY; printf '# Dummy task\nCreate hello.txt containing hi.\n' > tools/orchestrate/wp/DUMMY/brief.md
git add .; git commit -qm baseline
PATH="$TMP/bin:$PATH"; mkdir -p "$TMP/bin"
cat > "$TMP/bin/hermes" <<'SH'
#!/usr/bin/env bash
set -e
prompt=''; indir=''
while (($#)); do case "$1" in -z) prompt=$2; shift 2;; --in) indir=$2; shift 2;; *) shift;; esac; done
printf 'hi\n' > "$indir/hello.txt"
echo 'RESULT: PASS'
SH
chmod +x "$TMP/bin/hermes"
bash tools/orchestrate/run-luna.sh DUMMY --test 'test -f hello.txt' --max-attempts 1 --paths 'hello.txt'
python3 - <<'PY'
import json,pathlib,subprocess
v=json.loads(pathlib.Path('tools/orchestrate/wp/DUMMY/verdict.json').read_text())
assert v['status']=='pass',v
assert subprocess.run(['git','-C','.worktrees/DUMMY','log','-1','--format=%s'],capture_output=True,text=True).stdout.startswith('wp(DUMMY): Dummy task')
PY
# Simulate the violation in a separate WP branch/worktree without moving main.
mkdir -p tools/orchestrate/wp/ESCAPE/attempts
printf '# Escape task\\n' > tools/orchestrate/wp/ESCAPE/brief.md
git worktree add -qb wp/ESCAPE .worktrees/ESCAPE main
printf 'changed\\n' > .worktrees/ESCAPE/outside.txt
set +e; bash tools/orchestrate/run-luna.sh ESCAPE --max-attempts 1 --paths 'allowed/**' >/dev/null 2>&1; rc=$?; set -e
[[ $rc -ne 0 ]]
python3 - <<'PY'
import json,pathlib
v=json.loads(pathlib.Path('tools/orchestrate/wp/ESCAPE/verdict.json').read_text())
assert v['status'] in ('fail','escalate'),v
assert 'outside.txt' in v['violations'],v
PY
printf 'selftest passed\n'
