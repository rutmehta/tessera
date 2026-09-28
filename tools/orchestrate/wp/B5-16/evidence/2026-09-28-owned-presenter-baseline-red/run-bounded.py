import os, signal, subprocess, sys, time, datetime
out_dir = sys.argv[1]
cmd = ["swift", "test", "--scratch-path", os.path.join(out_dir, "scratch"), "--jobs", "2", "-c", "release", "-Xswiftc", "-enable-testing", "--filter", "DocumentSaveDismissAttachmentTests/testKnownGapEntireAttachmentAfterCancelWithoutAnyObservationNeedsProgress"]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
with open(os.path.join(out_dir, "swift-test.raw.log"), "wb") as log:
    p = subprocess.Popen(cmd, cwd="/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera/apps/mac", stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    with open(os.path.join(out_dir, "process.txt"), "w") as f: f.write(f"pid={p.pid}\npgid={os.getpgid(p.pid)}\nstarted_utc={started}\ncommand={' '.join(cmd)}\n")
    try:
        rc = p.wait(timeout=600)
        timed_out = False
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(p.pid, signal.SIGTERM)
        try: rc = p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            rc = p.wait()
    ended = datetime.datetime.now(datetime.timezone.utc).isoformat()
with open(os.path.join(out_dir, "direct-exit.txt"), "w") as f:
    f.write(f"child_exit={rc}\nwatchdog_timeout={str(timed_out).lower()}\nended_utc={ended}\n")
sys.exit(124 if timed_out else rc)
