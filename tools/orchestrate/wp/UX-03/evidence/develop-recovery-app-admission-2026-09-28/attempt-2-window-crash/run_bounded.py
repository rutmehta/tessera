import os, signal, subprocess, sys, time
root = '/Users/rutmehta/.codex/worktrees/review-ownership/tessera'
out = '/Volumes/betterSSD/tessera-cache/swift/develop-recovery-app-admission-cf4f4114'
command = ['swift', 'test', '--package-path', 'apps/mac', '--scratch-path', out + '/scratch', '-c', 'release', '--jobs', '2', '-Xswiftc', '-enable-testing', '--filter', 'DevelopRecovery|ThumbnailFlightDrain|RecoveryWindowCloseGuard|OutputAdmissionCancellation']
with open(out + '/swift-test-release.log', 'wb') as log:
    child = subprocess.Popen(command, cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    open(out + '/child-pid.txt', 'w').write(str(child.pid) + '\n')
    try:
        code = child.wait(timeout=300)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
        code = 124
    open(out + '/direct-exit.txt', 'w').write(f'exit={code}\nfinished={time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}\n')
    sys.exit(code)
