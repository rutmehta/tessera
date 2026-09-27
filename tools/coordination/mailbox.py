#!/usr/bin/env python3
"""Durable, single-writer Git mailbox. Never executes message contents or wakes Codex."""
import argparse
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid

LIMIT = 16384
TERMINAL = {"completed", "failed", "needs_user"}


class Mailbox:
    def __init__(self, remote, cache, machine, clock=time.time):
        if machine not in {"A", "B"} or not remote or remote.startswith("-"):
            raise ValueError("machine must be A/B and remote must be an explicit Git URL/path")
        self.remote, self.cache, self.machine, self.clock = remote, Path(cache).expanduser(), machine, clock
        self.peer = "B" if machine == "A" else "A"
        self.repo = self.cache / "repo.git"
        self.heads = {}

    def git(self, *args, data=None, env=None):
        proc = subprocess.run(["git", "--git-dir", str(self.repo), *args], input=data,
                              text=True, capture_output=True, timeout=60,
                              env={**os.environ, "GIT_TERMINAL_PROMPT": "0", **(env or {})})
        if proc.returncode:
            raise RuntimeError(f"git {args[0]} failed: {proc.stderr.strip()[:1500]}")
        return proc.stdout.strip()

    @staticmethod
    def branch(machine):
        return f"refs/heads/codex/coordination-{machine.lower()}"

    @contextmanager
    def operation(self):
        self.cache.mkdir(parents=True, exist_ok=True)
        with (self.cache / "lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            identity = {"remote": self.remote, "machine": self.machine}
            identity_path = self.cache / "identity.json"
            if identity_path.exists() and json.loads(identity_path.read_text()) != identity:
                raise ValueError("cache is bound to a different remote or machine; use another cache")
            if not self.repo.exists():
                subprocess.run(["git", "init", "--bare", "--quiet", str(self.repo)], check=True,
                               capture_output=True, timeout=20)
                self.git("remote", "add", "origin", self.remote)
            if self.git("remote", "get-url", "origin") != self.remote:
                raise ValueError("cache origin does not match requested remote")
            identity_path.write_text(json.dumps(identity) + "\n")
            refs = self.git("ls-remote", "--heads", "origin", self.branch("A"), self.branch("B"))
            found = dict(line.split("\t", 1)[::-1] for line in refs.splitlines())
            for machine in ("A", "B"):
                branch = self.branch(machine)
                tracking = f"refs/remotes/origin/{machine}"
                previous = self.git("for-each-ref", "--format=%(objectname)", tracking)
                if branch in found:
                    # Remote-tracking refs accept rewinds by default in Git. Fetch
                    # to FETCH_HEAD, verify ancestry, then advance our durable ref.
                    self.git("fetch", "--quiet", "--no-tags", "origin", branch)
                    current = self.git("rev-parse", "FETCH_HEAD")
                    if previous:
                        self.git("merge-base", "--is-ancestor", previous, current)
                    self.git("update-ref", tracking, current)
                    self.heads[machine] = current
                else:
                    if previous:
                        raise ValueError(f"remote mailbox branch disappeared: {branch}")
                    self.heads[machine] = None
            yield

    @staticmethod
    def valid_id(value):
        if not isinstance(value, str) or str(uuid.UUID(value)) != value:
            raise ValueError("message ID must be a canonical lowercase UUID")
        return value

    @staticmethod
    def bounded(value, name):
        if not isinstance(value, str) or not value.strip() or len(value.encode()) > LIMIT:
            raise ValueError(f"{name} must contain 1–{LIMIT} UTF-8 bytes")
        return value

    def records(self, machine, folder):
        head = self.heads[machine]
        if head is None:
            return {}, []
        paths = self.git("ls-tree", "-r", "--name-only", head, "--", folder).splitlines()
        if len(paths) > 5000:
            raise ValueError("mailbox exceeds 5000 records; archive before continuing")
        records, invalid = {}, []
        for path in paths:
            try:
                ident = self.valid_id(Path(path).stem)
                if path != f"{folder}/{ident}.json":
                    raise ValueError("invalid record path")
                blob = f"{head}:{path}"
                if int(self.git("cat-file", "-s", blob)) > LIMIT + 4096:
                    raise ValueError("oversized record")
                item = json.loads(self.git("show", blob))
                if item.get("version") != 1 or item.get("id") != ident:
                    raise ValueError("invalid schema or message ID")
                if item.get("from") != machine or item.get("to") != ("B" if machine == "A" else "A"):
                    raise ValueError("sender/recipient does not match branch ownership")
                if folder == "messages":
                    if item.get("kind") not in {"request", "status", "result"}:
                        raise ValueError("invalid message kind")
                    self.bounded(item.get("body"), "body")
                    if not isinstance(item.get("target_chat"), str):
                        raise ValueError("missing chat identifier")
                    created, expires = item.get("created_at"), item.get("expires_at")
                    if type(created) is not int or type(expires) is not int or not 0 < expires - created <= 604800:
                        raise ValueError("invalid validity window")
                    if item.get("reply_to") is not None:
                        self.valid_id(item["reply_to"])
                else:
                    if item.get("status") not in TERMINAL | {"accepted"}:
                        raise ValueError("invalid receipt status")
                    self.bounded(item.get("summary"), "summary")
                    if type(item.get("updated_at")) is not int:
                        raise ValueError("invalid receipt timestamp")
                records[ident] = item
            except (ValueError, TypeError, AttributeError, KeyError) as exc:
                invalid.append({"machine": machine, "path": path, "error": str(exc)})
        return records, invalid

    def publish(self, folder, item):
        parent = self.heads[self.machine]
        with tempfile.TemporaryDirectory(dir=self.cache) as tmp:
            env = {"GIT_INDEX_FILE": str(Path(tmp) / "index"),
                   "GIT_AUTHOR_NAME": f"Tessera Machine {self.machine}",
                   "GIT_AUTHOR_EMAIL": f"tessera-machine-{self.machine.lower()}@localhost",
                   "GIT_COMMITTER_NAME": f"Tessera Machine {self.machine}",
                   "GIT_COMMITTER_EMAIL": f"tessera-machine-{self.machine.lower()}@localhost"}
            self.git("read-tree", parent or "--empty", env=env)
            blob = self.git("hash-object", "-w", "--stdin", data=json.dumps(item, sort_keys=True) + "\n")
            self.git("update-index", "--add", "--cacheinfo", "100644", blob,
                     f"{folder}/{item['id']}.json", env=env)
            tree = self.git("write-tree", env=env)
            args = ["commit-tree", tree] + (["-p", parent] if parent else [])
            commit = self.git(*args, data=f"mailbox {self.machine}: {folder} {item['id']}\n", env=env)
            # A stale competing writer causes non-fast-forward rejection. Never force.
            self.git("push", "--quiet", "origin", f"{commit}:{self.branch(self.machine)}")
            self.git("update-ref", f"refs/remotes/origin/{self.machine}", commit)
            self.heads[self.machine] = commit
        return item

    def send(self, body, *, ttl=86400, message_id=None, kind="request", target_chat="", reply_to=None):
        self.bounded(body, "body")
        if type(ttl) is not int or not 1 <= ttl <= 604800 or kind not in {"request", "status", "result"}:
            raise ValueError("TTL must be 1–604800 seconds; kind is request/status/result")
        if not isinstance(target_chat, str) or len(target_chat.encode()) > 1024:
            raise ValueError("chat identifier must be at most 1024 UTF-8 bytes")
        ident = self.valid_id(message_id or str(uuid.uuid4()))
        if reply_to is not None:
            self.valid_id(reply_to)
        with self.operation():
            old, invalid = self.records(self.machine, "messages")
            if invalid:
                raise ValueError("own branch contains invalid messages; inspect before writing")
            if ident in old:
                existing = old[ident]
                if any(existing[k] != v for k, v in {"body": body, "kind": kind, "target_chat": target_chat,
                                                     "reply_to": reply_to}.items()):
                    raise ValueError("message ID already exists with different content")
                return existing
            now = int(self.clock())
            return self.publish("messages", {"version": 1, "id": ident, "from": self.machine,
                                "to": self.peer, "kind": kind, "body": body, "target_chat": target_chat,
                                "created_at": now, "expires_at": now + ttl, "reply_to": reply_to})

    def poll(self):
        with self.operation():
            incoming, bad1 = self.records(self.peer, "messages")
            processed, bad2 = self.records(self.machine, "receipts")
            received, bad3 = self.records(self.peer, "receipts")
            sent, bad4 = self.records(self.machine, "messages")
            now = int(self.clock())
            return {"machine": self.machine, "messages": [m for ident, m in incoming.items()
                    if ident not in processed and m["created_at"] <= now < m["expires_at"]],
                    "in_progress": [r for r in processed.values() if r["status"] == "accepted"],
                    "receipts": [r for ident, r in received.items() if ident in sent],
                    "expired": [ident for ident, m in incoming.items() if now >= m["expires_at"] and ident not in processed],
                    "invalid": bad1 + bad2 + bad3 + bad4,
                    "heads": self.heads.copy()}

    def receipt(self, ident, status, summary):
        self.valid_id(ident)
        self.bounded(summary, "summary")
        if status not in TERMINAL | {"accepted"}:
            raise ValueError("status must be accepted/completed/failed/needs_user")
        with self.operation():
            incoming, invalid = self.records(self.peer, "messages")
            old, own_invalid = self.records(self.machine, "receipts")
            if ident not in incoming or own_invalid:
                raise ValueError("no valid incoming message, or own receipts need repair")
            msg, previous, now = incoming[ident], old.get(ident), int(self.clock())
            if previous and previous["status"] in TERMINAL:
                if previous["status"] == status and previous["summary"] == summary:
                    return previous
                raise ValueError("terminal receipt is immutable; send a new message for follow-up")
            if status == "accepted" and not msg["created_at"] <= now < msg["expires_at"]:
                raise ValueError("cannot accept expired/future message")
            if status in {"completed", "failed"} and not previous:
                raise ValueError("publish accepted receipt before acting")
            item = {"version": 1, "id": ident, "from": self.machine, "to": self.peer,
                    "status": status, "summary": summary, "updated_at": now}
            if previous and previous["status"] == status and previous["summary"] == summary:
                return previous
            return self.publish("receipts", item)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--remote", required=True, help="exact existing Git remote URL")
    parser.add_argument("--cache", required=True, help="isolated cache directory, never a worktree")
    parser.add_argument("--machine", choices=["A", "B"], required=True)
    commands = parser.add_subparsers(dest="command", required=True)
    send = commands.add_parser("send")
    send.add_argument("--body-file", required=True, help="UTF-8 text file; '-' reads stdin")
    send.add_argument("--id", default=None, help="stable UUID for safe retry")
    send.add_argument("--ttl", type=int, default=86400)
    send.add_argument("--kind", choices=["request", "status", "result"], default="request")
    send.add_argument("--target-chat", default="")
    send.add_argument("--reply-to", default=None)
    commands.add_parser("poll")
    receipt = commands.add_parser("receipt")
    receipt.add_argument("--id", required=True)
    receipt.add_argument("--status", required=True, choices=sorted(TERMINAL | {"accepted"}))
    receipt.add_argument("--summary", required=True)
    args = parser.parse_args()
    box = Mailbox(args.remote, args.cache, args.machine)
    if args.command == "send":
        body = sys.stdin.read(LIMIT + 1) if args.body_file == "-" else Path(args.body_file).read_text()
        # Print/reuse this ID even if transport fails; never invent a new ID on retry.
        ident = args.id or str(uuid.uuid4())
        print(json.dumps({"attempt_id": ident}), file=sys.stderr)
        result = box.send(body, ttl=args.ttl, message_id=ident, kind=args.kind,
                          target_chat=args.target_chat, reply_to=args.reply_to)
    elif args.command == "receipt":
        result = box.receipt(args.id, args.status, args.summary)
    else:
        result = box.poll()
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        print(json.dumps({"error": str(error)}), file=sys.stderr)
        sys.exit(1)
