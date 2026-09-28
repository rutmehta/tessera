#!/usr/bin/env python3
"""Externally bound launcher for the filtered P11 capability helper.

This wrapper does not launch or terminate Tessera, request permissions, or
interpret captured pixels. It bounds only the explicitly supplied helper.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import pathlib
import plistlib
import subprocess
import sys
import tempfile
import time
from typing import Any


BETTER_SSD = pathlib.Path("/Volumes/betterSSD")
PROCESS_TIMEOUT_SECONDS = 15.0
MAX_RECORDED_OUTPUT_BYTES = 16_384
POST_KILL_REAP_SECONDS = 2.0
SCOPE = "filtered ScreenCaptureKit capability helper only; not P11 acceptance"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: os.PathLike[str] | str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _canonical(path: pathlib.Path, *, strict: bool) -> pathlib.Path:
    return path.expanduser().resolve(strict=strict)


def _inside(path: pathlib.Path, root: pathlib.Path) -> bool:
    try:
        path.relative_to(root)
        return path != root
    except ValueError:
        return False


def _number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(float(value))


def _positive_integer(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def _load_and_validate_config(
    raw: bytes, *, allowed_root: pathlib.Path
) -> tuple[dict[str, Any], dict[str, Any]]:
    try:
        config = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"invalid JSON configuration: {exc}") from exc
    if not isinstance(config, dict):
        raise ValueError("configuration must be a JSON object")

    for key in ("output_dir", "bundle_id", "app_path"):
        if not isinstance(config.get(key), str) or not config[key]:
            raise ValueError(f"{key} must be a nonempty string")
    for key in ("pid", "window_id", "display_id"):
        if not _positive_integer(config.get(key)):
            raise ValueError(f"{key} must be a positive integer")
    if not _number(config.get("launch_epoch")) or float(config["launch_epoch"]) <= 0:
        raise ValueError("launch_epoch must be a positive finite number")

    bundle_id = config["bundle_id"]
    if not bundle_id.startswith("dev.tessera.m258."):
        raise ValueError("bundle_id is outside the isolated dev.tessera.m258.* namespace")

    app_path_input = pathlib.Path(config["app_path"])
    if app_path_input.is_symlink():
        raise ValueError("app_path must not be a symlink")
    app_path = _canonical(app_path_input, strict=True)
    if not app_path.is_dir() or app_path.suffix != ".app" or not _inside(app_path, allowed_root):
        raise ValueError("app_path must be an existing .app bundle inside the isolated BetterSSD root")
    info_path = app_path / "Contents" / "Info.plist"
    try:
        with info_path.open("rb") as stream:
            info = plistlib.load(stream)
    except (OSError, plistlib.InvalidFileException) as exc:
        raise ValueError(f"app bundle Info.plist is unavailable or invalid: {exc}") from exc
    if info.get("CFBundleIdentifier") != bundle_id:
        raise ValueError("bundle_id does not match the supplied app bundle Info.plist")

    output_input = pathlib.Path(config["output_dir"])
    if output_input.exists() or output_input.is_symlink():
        raise ValueError("output_dir must not already exist")
    output_dir = _canonical(output_input, strict=False)
    if not _inside(output_dir, allowed_root):
        raise ValueError("output_dir must be a new directory inside the isolated BetterSSD root")
    if not output_dir.parent.is_dir():
        raise ValueError("output_dir parent must already exist")

    roi = config.get("roi_display_points")
    if not isinstance(roi, dict):
        raise ValueError("roi_display_points must be an object")
    normalized_roi: dict[str, float | int] = {}
    for key in ("x", "y", "width", "height"):
        value = roi.get(key)
        if not _number(value):
            raise ValueError(f"roi_display_points.{key} must be finite numeric")
        normalized_roi[key] = value
    if (
        float(normalized_roi["x"]) < 0
        or float(normalized_roi["y"]) < 0
        or float(normalized_roi["width"]) <= 0
        or float(normalized_roi["height"]) <= 0
        or float(normalized_roi["width"]) > 512
        or float(normalized_roi["height"]) > 512
    ):
        raise ValueError("ROI must be nonnegative, nonempty, and no larger than 512 by 512 points")

    # Keep the exact identity/geometry supplied by the caller in the receipt.
    scope = {
        "pid": config["pid"],
        "bundle_id": bundle_id,
        "app_path": str(app_path),
        "launch_epoch": float(config["launch_epoch"]),
        "window_id": config["window_id"],
        "display_id": config["display_id"],
        "roi_display_points": normalized_roi,
    }
    validated = dict(config)
    validated["app_path"] = str(app_path)
    validated["output_dir"] = str(output_dir)
    validated["launch_epoch"] = float(config["launch_epoch"])
    return validated, scope


def _write_record(path: pathlib.Path, record: dict[str, Any], *, initial: bool = False) -> None:
    encoded = (json.dumps(record, indent=2, sort_keys=True) + "\n").encode("utf-8")
    if initial:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        return
    temporary: str | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", delete=False) as stream:
            temporary = stream.name
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def _exclusive_write(path: pathlib.Path, content: bytes) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())


def _stop_owned_child(process: subprocess.Popen[bytes]) -> tuple[bool, bool, str | None]:
    """Request termination and boundedly reap only the Popen child."""
    kill_requested = False
    if process.poll() is None:
        try:
            process.kill()
            kill_requested = True
        except ProcessLookupError:
            pass
        except OSError as exc:
            return kill_requested, False, str(exc)
    try:
        process.wait(timeout=POST_KILL_REAP_SECONDS)
        return kill_requested, True, None
    except subprocess.TimeoutExpired:
        return kill_requested, False, "owned helper did not exit within bounded reap interval"
    except OSError as exc:
        return kill_requested, False, str(exc)


def execute(
    helper: os.PathLike[str] | str,
    config_path: os.PathLike[str] | str,
    record_path: os.PathLike[str] | str,
    *,
    allowed_root: os.PathLike[str] | str = BETTER_SSD,
    timeout: float = PROCESS_TIMEOUT_SECONDS,
) -> int:
    """Run exactly one helper with an external wall-clock bound.

    `allowed_root` and `timeout` are injectable only for isolated unit tests;
    the CLI uses the fixed BetterSSD root and timeout.
    """
    helper_input = pathlib.Path(helper)
    config_input = pathlib.Path(config_path)
    receipt_input = pathlib.Path(record_path)
    root = _canonical(pathlib.Path(allowed_root), strict=True)
    source_path = pathlib.Path(__file__).resolve(strict=True)
    started_at = time.time()

    if receipt_input.exists() or receipt_input.is_symlink():
        raise FileExistsError(f"refusing to overwrite launcher record: {receipt_input}")
    receipt = receipt_input.absolute()
    if not receipt.parent.is_dir():
        raise ValueError("launcher record parent must already exist")

    input_hashes: dict[str, str | None] = {"launcher_sha256": None, "helper_sha256": None, "config_sha256": None}
    try:
        input_hashes["launcher_sha256"] = sha256_file(source_path)
        if helper_input.is_symlink():
            raise ValueError("helper must not be a symlink")
        helper_path = _canonical(helper_input, strict=True)
        if not helper_path.is_file() or not os.access(helper_path, os.X_OK):
            raise ValueError("helper must be an existing executable file")
        input_hashes["helper_sha256"] = sha256_file(helper_path)
        if config_input.is_symlink():
            raise ValueError("configuration must not be a symlink")
        config_path_canonical = _canonical(config_input, strict=True)
        if not config_path_canonical.is_file():
            raise ValueError("configuration must be an existing regular file")
        config_bytes = config_path_canonical.read_bytes()
        input_hashes["config_sha256"] = sha256_bytes(config_bytes)
    except (OSError, ValueError) as exc:
        base = {
            "scope": SCOPE,
            "outcome": "preflight_error",
            "error": str(exc),
            "direct_exit": None,
            "launcher_exit": 2,
            "started_epoch": started_at,
            **input_hashes,
        }
        _write_record(receipt, base, initial=True)
        return 2

    base: dict[str, Any] = {
        "scope": SCOPE,
        "outcome": "starting",
        "command": [str(helper_path), "<immutable-config-snapshot>"],
        "shell": False,
        "timeout_seconds": timeout,
        "started_epoch": started_at,
        **input_hashes,
    }
    _write_record(receipt, base, initial=True)

    try:
        validated, scope = _load_and_validate_config(config_bytes, allowed_root=root)
    except (OSError, ValueError) as exc:
        base.update(outcome="preflight_error", error=str(exc), direct_exit=None, launcher_exit=2, completed_epoch=time.time())
        _write_record(receipt, base)
        return 2

    base["scope"] = {"label": SCOPE, **scope}
    base["configuration"] = validated

    snapshot_bytes = (json.dumps(validated, indent=2, sort_keys=True) + "\n").encode("utf-8")
    snapshot_path = pathlib.Path(f"{receipt}.config.json")
    try:
        _exclusive_write(snapshot_path, snapshot_bytes)
    except OSError as exc:
        base.update(outcome="preflight_error", error=f"cannot preserve immutable config snapshot: {exc}", direct_exit=None, launcher_exit=2)
        _write_record(receipt, base)
        return 2
    base["configuration_snapshot"] = str(snapshot_path)
    base["config_snapshot_sha256"] = sha256_bytes(snapshot_bytes)

    # The helper receives the preserved immutable canonical config snapshot,
    # while the receipt separately hashes the caller's original file before
    # and after the invocation.
    process: subprocess.Popen[bytes] | None = None
    stdout_path = pathlib.Path(f"{receipt}.stdout.log")
    stderr_path = pathlib.Path(f"{receipt}.stderr.log")
    if stdout_path.exists() or stderr_path.exists():
        base.update(outcome="preflight_error", error="launcher stdout/stderr files must not already exist", direct_exit=None, launcher_exit=2)
        _write_record(receipt, base)
        return 2
    try:
        with open(stdout_path, "xb") as stdout_file, open(stderr_path, "xb") as stderr_file:
            deadline = time.monotonic() + timeout
            base["command"] = [str(helper_path), str(snapshot_path)]
            process = subprocess.Popen(
                [str(helper_path), str(snapshot_path)],
                stdin=subprocess.DEVNULL,
                stdout=stdout_file,
                stderr=stderr_file,
                shell=False,
                close_fds=True,
            )
            base["helper_pid"] = process.pid
            try:
                _write_record(receipt, base)
            except OSError:
                kill_requested, reaped, stop_error = _stop_owned_child(process)
                base.update(
                    outcome="launch_error",
                    error="could not record spawned helper identity",
                    kill_requested=kill_requested,
                    helper_reaped=reaped,
                    cleanup_error=stop_error,
                    direct_exit=None,
                    launcher_exit=125 if not reaped else 2,
                )
                _write_record(receipt, base)
                return 125 if not reaped else 2
            try:
                process.wait(timeout=max(0.0, deadline - time.monotonic()))
                direct_exit = process.returncode
                outcome = "exited"
                timeout_action = None
                cleanup_incomplete = False
                kill_requested = False
                helper_reaped = True
                stop_error = None
            except subprocess.TimeoutExpired:
                # This is the only process we own here. Never match by process
                # name, signal a process group, or address the configured app PID.
                kill_requested, helper_reaped, stop_error = _stop_owned_child(process)
                cleanup_incomplete = not helper_reaped
                direct_exit = None
                outcome = "timeout"
                if kill_requested:
                    timeout_action = "killed_owned_helper_process_only"
                elif helper_reaped:
                    timeout_action = "owned_helper_already_exited"
                else:
                    timeout_action = "owned_helper_termination_unconfirmed"

        try:
            stdout = _tail_file(stdout_path)
        except OSError as exc:
            stdout = b""
            base.setdefault("postrun_output_errors", {})["stdout"] = str(exc)
        try:
            stderr = _tail_file(stderr_path)
        except OSError as exc:
            stderr = b""
            base.setdefault("postrun_output_errors", {})["stderr"] = str(exc)
        after_hashes: dict[str, str | None] = {}
        for label, path in (
            ("launcher_sha256_after", source_path),
            ("helper_sha256_after", helper_path),
            ("config_sha256_after", config_path_canonical),
            ("config_snapshot_sha256_after", snapshot_path),
        ):
            try:
                after_hashes[label] = sha256_file(path)
            except OSError as exc:
                after_hashes[label] = None
                base.setdefault("postrun_hash_errors", {})[label] = str(exc)
        base.update(
            outcome=outcome,
            direct_exit=direct_exit,
            timeout_action=timeout_action,
            kill_requested=kill_requested,
            stdout_tail=stdout.decode("utf-8", errors="replace"),
            stderr_tail=stderr.decode("utf-8", errors="replace"),
            stdout_file=str(stdout_path),
            stderr_file=str(stderr_path),
            helper_reaped=helper_reaped,
            cleanup_error=stop_error,
            completed_epoch=time.time(),
            elapsed_seconds=time.time() - started_at,
            **after_hashes,
        )
        if (
            base["launcher_sha256_after"] != base["launcher_sha256"]
            or base["helper_sha256_after"] != base["helper_sha256"]
            or base["config_sha256_after"] != base["config_sha256"]
            or base["config_snapshot_sha256_after"] != base["config_snapshot_sha256"]
        ):
            base["integrity_error"] = "launcher/helper/original config/passed config snapshot changed during bounded invocation"
        _write_record(receipt, base)
        if cleanup_incomplete:
            base["cleanup_error"] = "owned helper did not exit within the bounded post-kill reap interval"
            base["launcher_exit"] = 125
            _write_record(receipt, base)
            return 125
        if "integrity_error" in base or base.get("postrun_hash_errors") or base.get("postrun_output_errors"):
            base["launcher_exit"] = 126
            _write_record(receipt, base)
            return 126
        launcher_exit = 124 if outcome == "timeout" else int(direct_exit)
        base["launcher_exit"] = launcher_exit
        _write_record(receipt, base)
        return launcher_exit
    except OSError as exc:
        kill_requested = False
        helper_reaped = process is None or process.poll() is not None
        cleanup_error = None
        if process is not None and not helper_reaped:
            kill_requested, helper_reaped, cleanup_error = _stop_owned_child(process)
        base.update(
            outcome="launch_error",
            error=str(exc),
            direct_exit=None,
            launcher_exit=2 if helper_reaped else 125,
            kill_requested=kill_requested,
            helper_reaped=helper_reaped,
            cleanup_error=cleanup_error,
            completed_epoch=time.time(),
        )
        _write_record(receipt, base)
        return 2 if helper_reaped else 125


def _tail_file(path: pathlib.Path) -> bytes:
    with path.open("rb") as stream:
        stream.seek(0, os.SEEK_END)
        size = stream.tell()
        stream.seek(max(0, size - MAX_RECORDED_OUTPUT_BYTES))
        return stream.read(MAX_RECORDED_OUTPUT_BYTES)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", required=True, help="explicit compiled filtered-capture helper")
    parser.add_argument("--config", required=True, help="explicit isolated capture configuration JSON")
    parser.add_argument("--record", required=True, help="new launcher receipt path; existing files are rejected")
    args = parser.parse_args(argv)
    try:
        return execute(args.helper, args.config, args.record)
    except (OSError, ValueError) as exc:
        print(f"launcher refused invocation: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
