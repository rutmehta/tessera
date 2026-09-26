"""Performance collection and fail-closed, host-specific regression gates."""
import math
import re
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]

FIELDS = {"bench", "fixture", "metric", "value", "unit", "backend", "host"}
CPU_FIXTURES = {
    "index-search": "100k-fts-v1",
    "sidecar-roundtrip": "recipe-xmp-fsync-v1",
    "preview-pyramid": "45mp-gradient-v1",
    "pipeline-cpu-l3": "rgb-2048x1536-scale8-v1",
    "export-web": "rgb-2048x1536-web-q85-v1",
    "compositor-20-layer": "rgba8-1024x768-l0-v1",
}


def key(row):
    return tuple(row[field] for field in sorted(FIELDS - {"value"}))


def validate(rows):
    if not isinstance(rows, list) or not rows:
        raise ValueError("results must be a nonempty array")
    seen = set()
    for row in rows:
        if not isinstance(row, dict) or set(row) != FIELDS:
            raise ValueError("result fields must match schema exactly")
        for field in FIELDS - {"value"}:
            if not isinstance(row[field], str) or not row[field].strip():
                raise ValueError(f"invalid {field}")
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", row["host"]):
            raise ValueError("host must be a safe path component")
        if row["unit"] != "ms" or row["backend"] not in {"cpu", "gpu"}:
            raise ValueError("unsupported unit or backend")
        v = row["value"]
        if type(v) not in (int, float) or not math.isfinite(v) or v <= 0:
            raise ValueError("timings must be finite positive numbers")
        if key(row) in seen:
            raise ValueError("duplicate measurement")
        seen.add(key(row))
    return rows


def compare(rows, baseline):
    validate(rows)
    if baseline.get("schema_version") != 1:
        raise ValueError("unsupported baseline version")
    old = {key(r): r for r in validate(baseline["results"])}
    if set(old) != {key(r) for r in rows}:
        raise ValueError("baseline measurement set/host differs; record and review this suite on this host")
    tolerances = baseline["tolerances"]
    lines = ["| Bench / fixture | Metric | Current ms | Baseline ms | Delta | Tolerance | Status |",
             "|---|---|---:|---:|---:|---:|---|"]
    failed = False
    for row in rows:
        t = tolerances.get(row["bench"] + "/" + row["metric"],
                           tolerances.get(row["metric"]))
        if type(t) not in (int, float) or not math.isfinite(t) or not 0 <= t <= 1:
            raise ValueError("missing or invalid tolerance (fraction in [0,1])")
        before = old[key(row)]["value"]
        delta = (row["value"] - before) / before
        regressed = delta > t and not math.isclose(delta, t, abs_tol=1e-12)
        failed |= regressed
        lines.append(f"| {row['bench']} / {row['fixture']} | {row['metric']} | "
                     f"{row['value']:.3f} | {before:.3f} | {delta:+.1%} | {t:.0%} | "
                     f"{'REGRESSION' if regressed else 'ok'} |")
    return "\n".join(lines), failed


def parse_gpu(name, text, host):
    rows = []

    def add(bench, fixture, metric, value):
        rows.append(dict(bench=bench, fixture=fixture, metric=metric,
                         value=float(value), unit="ms", backend="gpu", host=host))

    if name == "develop":
        if not re.search(r"Metal \(.+\) panels, screen L2", text):
            raise ValueError("Develop did not report a Metal backend")
        matches = re.findall(r"tone exposure\s+L(\d+): median ([\d.]+) ms, p90 ([\d.]+) ms", text)
        if len(matches) != 1:
            raise ValueError("missing/duplicate Develop timing")
        level, median, p90 = matches[0]
        for metric, value in [("median_ms", median), ("p90_ms", p90)]:
            add("develop-slider", f"sony-arw.ARW-tone-L{level}-v1", metric, value)
    elif name == "export":
        matches = re.findall(r"BENCH file=sony-arw.ARW preset=web requested=gpu used_gpu=true lens_off=false seconds=([\d.]+)", text)
        if len(matches) != 1:
            raise ValueError("GPU export missing or fell back to CPU")
        add("export-web", "sony-arw.ARW-full-development-web-v1", "elapsed_ms", float(matches[0]) * 1000)
    elif name == "resident":
        for bench, prefix in [("resident-l2", r"L2 full recomposite"),
                              ("resident-dab", r"64² dab → L2 recomposite"),
                              ("resident-l0", r"L0 full composite")]:
            matches = re.findall(prefix + r"[^\n]*?median ([\d.]+) ms", text)
            if len(matches) != 1:
                raise ValueError(f"missing/duplicate {bench} timing")
            add(bench, "100-layer-20mp-v1", "median_ms", matches[0])
    else:
        raise ValueError(f"unknown GPU adapter {name}")
    return validate(rows)


def command(args, env, log, timeout=540.0):
    """Kill the whole test process tree on timeout, not just Cargo."""
    with log.open("w") as output:
        process = subprocess.Popen(args, cwd=ROOT, env=env, stdout=output,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            raise ValueError(f"benchmark timed out; see {log}") from None
    text = log.read_text()
    if code:
        raise ValueError(f"command failed ({code}); see {log}\n{text[-2000:]}")
    return text


def host_id():
    cpu = platform.machine()
    if sys.platform == "darwin":
        cpu = subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip()
    label = os.environ.get("BENCH_HOST", platform.node().split(".")[0])
    return re.sub(r"[^A-Za-z0-9._-]", "-",
                  f"{label}-{platform.system()}-{platform.release().split('.')[0]}-{cpu}-threads4")


def collect(suite, worker, host, directory):
    env = {k: v for k, v in os.environ.items() if not k.startswith(
        ("TESSERA_BENCH", "TESSERA_EXPORT", "TESSERA_RENDER", "PIPELINE_RAW"))}
    env.pop("DYLD_FALLBACK_LIBRARY_PATH", None)
    env.update(BENCH_HOST=host, RAYON_NUM_THREADS="4", RUST_TEST_THREADS="1",
               TESSERA_EXPORT_BACKEND="cpu")
    if suite == "cpu":
        if not worker:
            raise ValueError("use cargo run --release -p tessera-bench -- ... to collect")
        return json.loads(command([worker, "--measure-cpu"], env, directory / "cpu.log"))
    target = Path(env.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    if target.is_relative_to(ROOT):
        raise ValueError("GPU builds require CARGO_TARGET_DIR outside the checkout")
    fixture = ROOT / "fixtures/raw/sony-arw.ARW"
    if not fixture.is_file() or [p.name for p in fixture.parent.iterdir()
                                  if p.suffix.lower() == ".arw"] != [fixture.name]:
        raise ValueError("GPU suite requires exactly fixtures/raw/sony-arw.ARW as its sole ARW; run fixtures/fetch.sh")
    env.update(TESSERA_BENCH_EXT="arw", TESSERA_BENCH_BACKENDS="gpu",
               TESSERA_BENCH_PANELS="tone exposure", TESSERA_BENCH_FILE=str(fixture),
               TESSERA_BENCH_PRESET="web", TESSERA_BENCH_WEB_SCALE="1",
               TESSERA_EXPORT_BACKEND="gpu")
    rows = []
    for name, package, binary, test in [
        ("develop", "tessera-ffi", "develop", "bench_panel_latency"),
        ("export", "export", "gpu_bench", "fixture_export_worker"),
        ("resident", "compositor", "bench", "resident_100_layers_20mp"),
    ]:
        text = command(["cargo", "test", "--locked", "--release", "-p", package,
                        "--test", binary, test, "--", "--ignored", "--exact",
                        "--nocapture", "--test-threads=1"], env, directory / f"{name}.log")
        rows.extend(parse_gpu(name, text, host))
    return rows


def check_suite(rows, suite, host):
    validate(rows)
    if any(r["host"] != host or r["backend"] != suite for r in rows):
        raise ValueError("host/backend mismatch")
    if suite == "cpu":
        expected = {(b, f, "median_ms") for b, f in CPU_FIXTURES.items()}
        actual = {(r["bench"], r["fixture"], r["metric"]) for r in rows}
    else:
        expected = {("develop-slider", "median_ms"), ("develop-slider", "p90_ms"),
                    ("export-web", "elapsed_ms"), ("resident-l2", "median_ms"),
                    ("resident-dab", "median_ms"), ("resident-l0", "median_ms")}
        actual = {(r["bench"], r["metric"]) for r in rows}
    if actual != expected or len(rows) != len(expected):
        raise ValueError("incomplete or unexpected benchmark suite")


def write_json(path, data):
    # Replace atomically only after every measurement and schema check succeeded.
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data, indent=2, allow_nan=False) + "\n")
    temporary.replace(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=["cpu", "gpu"], default="cpu")
    parser.add_argument("--host", default=host_id(), help="stable hardware/OS label; default auto-detected")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--record-baseline", action="store_true")
    parser.add_argument("--input", type=Path, help="validate/compare saved results instead of measuring")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "bench-results")
    parser.add_argument("--worker", help=argparse.SUPPRESS)
    args = parser.parse_args()
    gate = os.environ.get("BENCH_GATE") == "1"
    if args.record_baseline and gate:
        raise ValueError("refusing --record-baseline with BENCH_GATE=1")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", args.host):
        raise ValueError("unsafe host identifier")
    directory = args.output_dir / args.host
    directory.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%S.%fZ")
    log_dir = directory / stamp
    log_dir.mkdir()
    rows = json.loads(args.input.read_text()) if args.input else collect(args.suite, args.worker, args.host, log_dir)
    check_suite(rows, args.suite, args.host)
    output = directory / f"{stamp}.json"
    write_json(output, rows)
    baseline = args.baseline or Path(__file__).parent / f"bench-baseline-{args.host}-{args.suite}.json"
    if args.record_baseline:
        document = {"schema_version": 1, "recorded_at": stamp,
                    "tolerances": {"median_ms": .15, "p90_ms": .20, "elapsed_ms": .15},
                    "results": rows}
        if baseline.exists():
            document["tolerances"] = json.loads(baseline.read_text())["tolerances"]
        table, _ = compare(rows, document)
        write_json(baseline, document)
        print(f"Recorded {baseline}; review before committing")
        status = 0
    elif baseline.exists():
        table, failed = compare(rows, json.loads(baseline.read_text()))
        status = 1 if gate and failed else 0
    else:
        table = "| Bench | Fixture | Metric | ms |\n|---|---|---|---:|\n" + "\n".join(
            f"| {r['bench']} | {r['fixture']} | {r['metric']} | {r['value']:.3f} |" for r in rows)
        table += f"\n\nNO BASELINE: {baseline}. Record on this host and review it."
        status = 2 if gate else 0
    print(table)
    output.with_suffix(".md").write_text(table + "\n")
    print(f"Results: {output}")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
            summary.write(table + "\n")
    return status


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f"tessera-bench: {error}", file=sys.stderr)
        sys.exit(2)
