"""Offline integration tests: real Git repos, no network or Codex processes."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


class MailboxTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.remote = self.root / "origin.git"
        subprocess.run(["git", "init", "--bare", "--quiet", str(self.remote)], check=True)
        spec = importlib.util.spec_from_file_location("tessera_mailbox", Path(__file__).with_name("mailbox.py"))
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.now = 1000000
        self.a = self.box("A", "a")
        self.b = self.box("B", "b")

    def box(self, machine, cache):
        return self.module.Mailbox(str(self.remote), self.root / cache, machine, clock=lambda: self.now)

    def test_delivery_replay_and_receipts_do_not_become_messages(self):
        msg = self.a.send("Please review commit abc", target_chat="Machine B")
        self.assertEqual([m["id"] for m in self.b.poll()["messages"]], [msg["id"]])
        self.b.receipt(msg["id"], "accepted", "Review starting")
        self.assertEqual(self.b.poll()["messages"], [])
        self.assertEqual(len(self.b.poll()["in_progress"]), 1)
        self.b.receipt(msg["id"], "completed", "Reviewed abc")
        state = self.a.poll()
        self.assertEqual(state["messages"], [])
        self.assertEqual(state["receipts"][0]["status"], "completed")
        self.assertEqual(self.box("B", "b-restart").poll()["messages"], [])
        with self.assertRaises(ValueError):
            self.b.receipt(msg["id"], "accepted", "Never reopen completed work")

    def test_same_id_is_idempotent_but_different_content_rejected(self):
        msg = self.a.send("review", message_id="12345678-1234-4234-8234-123456789abc")
        self.assertEqual(self.a.send("review", message_id=msg["id"]), msg)
        with self.assertRaises(ValueError):
            self.a.send("different action", message_id=msg["id"])
        self.assertEqual(len(self.b.poll()["messages"]), 1)

    def test_expiry_never_delivers_or_accepts_old_request(self):
        msg = self.a.send("old request", ttl=1)
        self.now += 2
        self.assertEqual(self.b.poll()["messages"], [])
        self.assertEqual(self.b.poll()["expired"], [msg["id"]])
        with self.assertRaises(ValueError):
            self.b.receipt(msg["id"], "accepted", "too late")

    def test_failed_push_does_not_claim_delivery_and_retry_keeps_id(self):
        original = self.a.git
        def fail_push(*args, **kwargs):
            if args[0] == "push":
                raise RuntimeError("simulated network failure")
            return original(*args, **kwargs)
        msg_id = "12345678-1234-4234-8234-123456789abc"
        with patch.object(self.a, "git", side_effect=fail_push):
            with self.assertRaises(RuntimeError):
                self.a.send("retry", message_id=msg_id)
        self.assertEqual(self.b.poll()["messages"], [])
        self.a.send("retry", message_id=msg_id)
        self.assertEqual([m["id"] for m in self.b.poll()["messages"]], [msg_id])

    def test_racing_writer_push_rejected_without_losing_first_message(self):
        self.a.send("seed")
        rival = self.box("A", "a-rival")
        original = self.a.git
        raced = False
        def race(*args, **kwargs):
            nonlocal raced
            if args[0] == "push" and not raced:
                raced = True
                rival.send("rival wins")
            return original(*args, **kwargs)
        with patch.object(self.a, "git", side_effect=race):
            with self.assertRaises(RuntimeError):
                self.a.send("must not overwrite rival")
        bodies = {m["body"] for m in self.b.poll()["messages"]}
        self.assertEqual(bodies, {"seed", "rival wins"})

    def test_shell_text_remains_data_and_invalid_inputs_rejected(self):
        marker = self.root / "MUST_NOT_EXIST"
        body = f"$(touch {marker}); `touch {marker}`"
        self.a.send(body)
        self.assertEqual(self.b.poll()["messages"][0]["body"], body)
        self.assertFalse(marker.exists())
        for kwargs in ({"message_id": "../../bad"}, {"ttl": 0}, {"ttl": 9999999}, {"kind": "receipt"}):
            with self.assertRaises(ValueError):
                self.a.send("bad", **kwargs)
        with self.assertRaises(ValueError):
            self.a.send("x" * 17000)

    def test_remote_history_rewind_or_deletion_is_reported(self):
        self.a.send("first")
        first = self.a.heads["A"]
        self.a.send("second")
        branch = self.a.branch("A")
        subprocess.run(["git", "--git-dir", str(self.remote), "update-ref", branch, first], check=True)
        with self.assertRaises((RuntimeError, ValueError)):
            self.a.poll()
        subprocess.run(["git", "--git-dir", str(self.remote), "update-ref", "-d", branch], check=True)
        with self.assertRaises((RuntimeError, ValueError)):
            self.a.poll()

    def test_wrong_sender_record_is_quarantined(self):
        msg = self.a.send("legitimate")
        with self.a.operation():
            corrupt = {**msg, "from": "B", "to": "A"}
            self.a.publish("messages", corrupt)
        result = self.b.poll()
        self.assertEqual(result["messages"], [])
        self.assertEqual(len(result["invalid"]), 1)
        with self.assertRaises(ValueError):
            self.b.receipt(msg["id"], "accepted", "must not accept")

    def test_cache_identity_cannot_switch_machine_or_remote(self):
        self.a.send("identity bound")
        with self.assertRaises(ValueError):
            self.box("B", "a").poll()
        with self.assertRaises(ValueError):
            self.module.Mailbox("/wrong/repo", self.root / "a", "A").poll()


if __name__ == "__main__":
    unittest.main()
