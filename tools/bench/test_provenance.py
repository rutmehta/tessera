"""Build-provenance tests: fixture artifacts only, no app launch or builds."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

# Do not create untracked bytecode beside the provenance helper under test.
sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "apps/mac/Support/provenance.py"


class ProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=ROOT / "apps/mac/Support")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.put("Cargo.toml", "[workspace]\n")
        self.put("apps/mac/Sources/Tessera/main.swift", "print(1)")
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Test", "-c",
                        "user.email=test@example.invalid", "commit", "-qm", "fixture"], check=True)
        self.put(".gitignore", "build/\ncache/\n")
        self.app = self.root / "build/Tessera.app"
        self.bin = self.root / "build/swift/release"
        self.target = self.root / "cache"
        self.put("build/Tessera.app/Contents/MacOS/Tessera", "after-install-name-tool")
        self.put("build/swift/release/Tessera", "original-linked-binary")
        self.put("build/swift/release/description.json", json.dumps({"swiftCommands": {
            name: {"moduleName": name, "otherArguments": ["-O", "-whole-module-optimization"]}
            for name in ("Tessera", "TesseraCore", "TesseraFFI")}}))
        self.put("cache/release/libtessera_ffi.a", "archive")
        self.put("apps/mac/build/ffi/libtessera_ffi.a", "archive")
        for generated, copied in (("TesseraFFI.swift", "TesseraFFI/TesseraFFI.swift"),
                                  ("CTesseraFFI.h", "CTesseraFFI/CTesseraFFI.h"),
                                  ("CTesseraFFI.modulemap", "CTesseraFFI/module.modulemap")):
            self.put("apps/mac/build/ffi/" + generated, generated)
            self.put("apps/mac/Sources/" + copied, generated)

    def put(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def load(self):
        self.assertTrue(SCRIPT.exists(), "provenance implementation is missing")
        spec = importlib.util.spec_from_file_location("provenance", SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def build(self, config="release"):
        p = self.load()
        snapshot = p.ffi_snapshot(self.root, self.target, p.snapshot(self.root))
        p.record(self.root, self.app, self.bin, config, self.target, snapshot)
        # Signing changes executable bytes, after bundled metadata is fixed.
        (self.app / "Contents/MacOS/Tessera").write_text("signed-executable")
        p.seal(self.app)
        return p

    def test_rejects_changed_inputs_or_outputs(self):
        p = self.build()
        targets = {
            "source": self.root / "apps/mac/Sources/Tessera/main.swift",
            "archive": self.root / "apps/mac/build/ffi/libtessera_ffi.a",
            "target archive": self.target / "release/libtessera_ffi.a",
            "binding": self.root / "apps/mac/Sources/TesseraFFI/TesseraFFI.swift",
            "generated binding": self.root / "apps/mac/build/ffi/CTesseraFFI.h",
            "signed binary": self.app / "Contents/MacOS/Tessera",
            "linked binary": self.bin / "Tessera",
            "metadata": self.app / "Contents/Resources/build-provenance.json",
            "receipt": p.receipt_path(self.app),
            "build description": self.bin / "description.json",
        }
        for label, path in targets.items():
            original = path.read_bytes()
            with self.subTest(label=label):
                path.write_bytes(original + (b"x" if label == "receipt" else b" "))
                with self.assertRaises(ValueError):
                    p.verify(self.root, self.app)
            path.write_bytes(original)
        added = self.put("apps/mac/Sources/Tessera/new.swift", "print(2)")
        with self.assertRaisesRegex(ValueError, "source"):
            p.verify(self.root, self.app)
        added.unlink()
        p.verify(self.root, self.app)

    def test_record_rejects_stale_or_mismatched_inputs(self):
        p = self.load()
        before = p.ffi_snapshot(self.root, self.target, p.snapshot(self.root))
        for name in ("apps/mac/Sources/Tessera/main.swift",
                     "apps/mac/build/ffi/libtessera_ffi.a",
                     "apps/mac/Sources/TesseraFFI/TesseraFFI.swift"):
            path = self.root / name
            original = path.read_bytes()
            with self.subTest(name=name):
                path.write_bytes(original + b"stale")
                with self.assertRaises(ValueError):
                    p.record(self.root, self.app, self.bin, "release", self.target, before)
            path.write_bytes(original)

    def test_release_rejects_unoptimized_or_incomplete_commands(self):
        p = self.load()
        desc_path = self.bin / "description.json"
        original = desc_path.read_text()
        for args in (["-Onone"], ["-O", "-Onone"], [], ["-g"]):
            with self.subTest(args=args):
                desc = json.loads(original)
                desc["swiftCommands"]["TesseraCore"]["otherArguments"] = args
                desc_path.write_text(json.dumps(desc))
                with self.assertRaises(ValueError):
                    p.record(self.root, self.app, self.bin, "release", self.target,
                             p.ffi_snapshot(self.root, self.target, p.snapshot(self.root)))
        for desc in ({}, {"swiftCommands": {}}, {"swiftCommands": {
                "only": {"moduleName": "Tessera", "otherArguments": ["-O"]}}}):
            with self.subTest(description=desc):
                desc_path.write_text(json.dumps(desc))
                with self.assertRaises(ValueError):
                    p.record(self.root, self.app, self.bin, "release", self.target,
                             p.ffi_snapshot(self.root, self.target, p.snapshot(self.root)))

    def test_debug_is_explicit_and_never_release_verified(self):
        desc = json.loads((self.bin / "description.json").read_text())
        for command in desc["swiftCommands"].values():
            command["otherArguments"] = ["-Onone", "-g"]
        (self.bin / "description.json").write_text(json.dumps(desc))
        p = self.build(config="debug")
        p.verify(self.root, self.app, require_release=False)
        with self.assertRaisesRegex(ValueError, "release"):
            p.verify(self.root, self.app)

    def test_rejects_matching_artifacts_replaced_during_swift_build(self):
        p = self.load()
        self.assertTrue(hasattr(p, "ffi_snapshot"), "missing pre-link FFI checkpoint")
        before = p.ffi_snapshot(self.root, self.target, p.snapshot(self.root))
        for path in ("cache/release/libtessera_ffi.a", "apps/mac/build/ffi/libtessera_ffi.a"):
            self.put(path, "matching-but-not-linked-archive")
        with self.assertRaisesRegex(ValueError, "archive"):
            p.record(self.root, self.app, self.bin, "release", self.target, before)

    def test_packaging_orders_fresh_release_build_and_postsign_receipt(self):
        script = (ROOT / "apps/mac/Support/make-app.sh").read_text()
        self.assertIn('CONFIG="${1:-release}"', script)
        ordered = ["provenance.py snapshot", "bash ./build-ffi.sh", "provenance.py ffi",
                   'swift build --scratch-path "$SCRATCH" -c "$CONFIG" --product Tessera',
                   "install_name_tool", "provenance.py record",
                   'codesign "${SIGN[@]}" --entitlements', "provenance.py seal",
                   "provenance.py verify"]
        indices = [script.index(text) for text in ordered]
        self.assertEqual(indices, sorted(indices))
        self.assertIn("mktemp -d", script)

    def test_cli_end_to_end_and_failure_exit(self):
        before = self.root / "build/snapshot.json"
        def run(command, *args):
            return subprocess.run([sys.executable, str(SCRIPT), command, "--root", str(self.root),
                                   *map(str, args)], text=True, capture_output=True)
        for command, args in (
            ("snapshot", ["--snapshot", before]),
            ("ffi", ["--target", self.target, "--snapshot", before]),
            ("record", ["--app", self.app, "--bin-dir", self.bin, "--target", self.target,
                        "--snapshot", before]),
            ("seal", ["--app", self.app]),
            ("verify", ["--app", self.app]),
        ):
            result = run(command, *args)
            self.assertEqual(result.returncode, 0, result.stderr)
        (self.app / "Contents/MacOS/Tessera").write_text("stale")
        result = run("verify", "--app", self.app)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("signed binary", result.stderr)

    def test_signed_bundle_round_trip(self):
        p = self.build()
        metadata = p.verify(self.root, self.app)
        self.assertEqual(metadata["configuration"], "release")
        self.assertEqual(len(metadata["commit"]), 40)
        self.assertEqual(len(metadata["archive_sha256"]), 64)
        self.assertEqual(len(metadata["bindings"]), 3)
        self.assertEqual(len(metadata["packaged_binary_sha256"]), 64)
        self.assertEqual(len(metadata["source_sha256"]), 64)

    def test_measurement_outputs_do_not_change_build_inputs(self):
        p = self.build()
        self.put("tools/orchestrate/wp/M2-53/run/trace.json", "{}")
        self.put("tools/orchestrate/wp/M2-53/RESULTS.md", "measurement")
        p.verify(self.root, self.app)


if __name__ == "__main__":
    unittest.main()
