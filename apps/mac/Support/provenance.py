#!/usr/bin/env python3
"""Content-addressed build receipt; see PROVENANCE.md for verification boundaries."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

BINDINGS = {
    "TesseraFFI.swift": "apps/mac/Sources/TesseraFFI/TesseraFFI.swift",
    "CTesseraFFI.h": "apps/mac/Sources/CTesseraFFI/CTesseraFFI.h",
    "CTesseraFFI.modulemap": "apps/mac/Sources/CTesseraFFI/module.modulemap",
}
ARCHIVE = "apps/mac/build/ffi/libtessera_ffi.a"
METADATA = "Contents/Resources/build-provenance.json"
BINARY = "Contents/MacOS/Tessera"


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def snapshot(root):
    # Include dirty and new nonignored files, not only HEAD. Generated bindings
    # are attested separately because build-ffi.sh deliberately overwrites them.
    names = git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z")
    files = {}
    for name in sorted(set(names.decode().split("\0")) - {""} - set(BINDINGS.values())):
        # Results/logs change while timing; they are not compiler or packaging inputs.
        if not (name.startswith(("crates/", ".cargo/", "apps/mac/Sources/", "apps/mac/Support/"))
                or name in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain",
                            "apps/mac/Package.swift", "apps/mac/Package.resolved", "apps/mac/build-ffi.sh"}):
            continue
        path = root / name
        files[name] = digest(path) if path.is_file() else "missing"
    return {"commit": git(root, "rev-parse", "HEAD").decode().strip(),
            "source_sha256": hashlib.sha256(encoded(files)).hexdigest(), "source_files": files}


def ffi_snapshot(root, target, before):
    equal(snapshot(root), {key: before[key] for key in
                          ("commit", "source_sha256", "source_files")}, "source during FFI build")
    artifacts = {"cargo_target_dir": str(target.resolve()),
                 "archive_sha256": digest(root / ARCHIVE),
                 "bindings": {name: digest(root / path) for name, path in BINDINGS.items()}}
    verify_ffi(root, artifacts)
    return {**before, **artifacts}


def record(root, app, bin_dir, configuration, target, before):
    equal(str(target.resolve()), before["cargo_target_dir"], "Cargo target directory")
    metadata = {"schema": 1, **before, "configuration": configuration,
                "rust_configuration": "release",
                "swift_bin_dir": str(bin_dir.resolve()),
                "swift_description_sha256": digest(bin_dir / "description.json"),
                "swift_description": json.loads((bin_dir / "description.json").read_text()),
                "linked_binary_sha256": digest(bin_dir / "Tessera"),
                "packaged_binary_sha256": digest(app / BINARY)}
    verify_inputs(root, metadata)
    if configuration == "release":
        verify_optimized(metadata)
    write_json(app / METADATA, metadata)
    return metadata


def receipt_path(app):
    return app.with_name(app.name + ".provenance.json")


def seal(app):
    # External receipt avoids a circular signature: nothing inside the signed
    # bundle is changed after codesign. Carry this sidecar with benchmark builds.
    write_json(receipt_path(app), {"schema": 1,
               "metadata_sha256": digest(app / METADATA),
               "signed_binary_sha256": digest(app / BINARY)})


def equal(actual, expected, label):
    if actual != expected:
        raise ValueError(f"{label} mismatch; rebuild before timing")


def verify_ffi(root, metadata):
    equal(digest(root / ARCHIVE), metadata["archive_sha256"], "archive")
    target = Path(metadata["cargo_target_dir"])
    equal(digest(target / "release/libtessera_ffi.a"), metadata["archive_sha256"], "target archive")
    for name, path in BINDINGS.items():
        equal(digest(root / path), metadata["bindings"][name], f"binding {name}")
        equal(digest(root / "apps/mac/build/ffi" / name), metadata["bindings"][name],
              f"generated binding {name}")


def verify_inputs(root, metadata):
    current = snapshot(root)
    for field in ("commit", "source_sha256", "source_files"):
        equal(current[field], metadata[field], field)
    verify_ffi(root, metadata)
    bin_dir = Path(metadata["swift_bin_dir"])
    equal(digest(bin_dir / "Tessera"), metadata["linked_binary_sha256"], "linked binary")
    equal(digest(bin_dir / "description.json"), metadata["swift_description_sha256"],
          "Swift build description")


def verify_optimized(metadata):
    equal(metadata["configuration"], "release", "release configuration")
    equal(metadata["rust_configuration"], "release", "Rust release configuration")
    commands = metadata["swift_description"].get("swiftCommands", {})
    modules = set()
    if not isinstance(commands, dict) or not commands:
        raise ValueError("missing Swift compiler commands")
    for command in commands.values():
        arguments = command.get("otherArguments", [])
        name = command.get("moduleName", "unknown")
        modules.add(name)
        if "-Onone" in arguments or not ({"-O", "-Osize", "-Ounchecked"} & set(arguments)):
            raise ValueError(f"Swift {name} is not optimized (or contains -Onone)")
    if not {"Tessera", "TesseraCore", "TesseraFFI"}.issubset(modules):
        raise ValueError("missing app Swift compiler commands")


def verify(root, app, require_release=True):
    receipt = json.loads(receipt_path(app).read_text())
    equal(receipt["schema"], 1, "receipt schema")
    equal(digest(app / METADATA), receipt["metadata_sha256"], "bundled metadata")
    equal(digest(app / BINARY), receipt["signed_binary_sha256"], "signed binary")
    metadata = json.loads((app / METADATA).read_text())
    equal(metadata["schema"], 1, "metadata schema")
    verify_inputs(root, metadata)
    if require_release or metadata["configuration"] == "release":
        verify_optimized(metadata)
    return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["snapshot", "ffi", "record", "seal", "verify"])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument("--app", type=Path)
    parser.add_argument("--bin-dir", type=Path)
    parser.add_argument("--target", type=Path)
    parser.add_argument("--snapshot", type=Path)
    parser.add_argument("--configuration", choices=["release", "debug"], default="release")
    parser.add_argument("--allow-debug", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "snapshot":
            write_json(args.snapshot, snapshot(args.root))
        elif args.command == "ffi":
            write_json(args.snapshot, ffi_snapshot(args.root, args.target,
                       json.loads(args.snapshot.read_text())))
        elif args.command == "record":
            record(args.root, args.app, args.bin_dir, args.configuration, args.target,
                   json.loads(args.snapshot.read_text()))
        elif args.command == "seal":
            seal(args.app)
        else:
            metadata = verify(args.root, args.app, not args.allow_debug)
            print(f"Verified {metadata['configuration']} {metadata['commit']}: {args.app}")
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"Provenance verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
