"""Link lane probes with Cargo's exact artifacts, without a new dependency edge."""
import json
import os
from pathlib import Path
import subprocess
import sys

mode, manifest = sys.argv[1:]
target = Path(os.environ["CARGO_TARGET_DIR"])
artifacts = {}
for line in Path(manifest).read_text().splitlines():
    item = json.loads(line)
    if item.get("reason") != "compiler-artifact":
        continue
    for path in item["filenames"]:
        if path.endswith(".rlib"):
            artifacts[item["target"]["name"]] = path


def externs(names):
    result = []
    for name in names:
        result.extend(["--extern", f"{name}={artifacts[name]}"])
    return result


link = ["-L", f"dependency={target / 'debug/deps'}", "-L", f"dependency={target}"]
if mode == "compat":
    baseline = target / "lr2-baseline-source"
    baseline.mkdir(exist_ok=True)
    archive = subprocess.run(
        ["git", "archive", "26e5cb8a", "crates/import-lrcat/src", "crates/sidecar/src"],
        check=True, capture_output=True,
    ).stdout
    subprocess.run(["tar", "-x", "-C", str(baseline)], input=archive, check=True)
    sidecar = target / "libsidecar_baseline.rlib"
    subprocess.run([
        "rustc", "--edition=2024", "--crate-type", "rlib", "--crate-name",
        "sidecar_baseline", str(baseline / "crates/sidecar/src/lib.rs"),
        *link, *externs(["engine_api", "serde", "serde_json", "quick_xml", "blake3"]),
        "-o", str(sidecar),
    ], check=True)
    output = target / "libimport_lrcat_baseline.rlib"
    subprocess.run([
        "rustc", "--edition=2024", "--crate-type", "rlib", "--crate-name",
        "import_lrcat_baseline", str(baseline / "crates/import-lrcat/src/lib.rs"),
        *link, *externs(["library", "engine_api", "rusqlite", "serde",
                        "serde_json", "tempfile", "roxmltree"]), "--extern", f"sidecar={sidecar}", "-o", str(output),
    ], check=True)
    args = [*externs(["import_lrcat", "engine_api", "serde_json", "rusqlite", "tempfile"]), "--extern", f"import_lrcat_baseline={output}"]
elif mode == "e2e":
    args = externs(["engine_api", "import_lrcat", "pipeline_cpu", "rusqlite", "tempfile"])
else:
    raise ValueError(f"unknown mode {mode}")
output = target / f"lr2-{mode}"
subprocess.run([
    "rustc", "--edition=2024", f"tools/orchestrate/wp/LR-2/{mode}.rs",
    *link, *args, "-o", str(output),
], check=True)
subprocess.run([str(output)], check=True)
