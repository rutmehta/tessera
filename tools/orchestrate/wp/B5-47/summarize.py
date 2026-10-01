#!/usr/bin/env python3
"""Summarize baseline differences; nested named spans are never added together."""
import json
from pathlib import Path

evidence = Path(__file__).resolve().parent / "evidence"
rows = []
for path in sorted(evidence.glob("*.trace.json.baselines.json")):
    if not path.name.startswith(("before-", "after-")):
        continue
    scenarios = {row["scenario"]: row for row in json.loads(path.read_text())}
    trace_path = Path(str(path)[:-len(".baselines.json")])
    trace = json.loads(trace_path.read_text())
    names = sorted({e["name"] for e in trace["events"]
                    if e["mainThread"] and e.get("durationMs") is not None and e["name"].endswith("_end")})
    spans = {}
    for name in names:
        values = [e["durationMs"] for e in trace["events"] if e["name"] == name and e["mainThread"]]
        spans[name] = {"count": len(values), "maxMs": max(values), "totalMs": sum(values)}
    export = scenarios["export"]
    deltas = {}
    for label in ["idle", "edit"]:
        base = scenarios[label]
        deltas[label] = {key: export[key] - base[key]
                         for key in ["busyMaxMs", "busyP95Ms", "busyMsPerSecond"]}
        deltas[label]["durationAdjustedTotalMs"] = export["busyTotalMs"] - base["busyMsPerSecond"] * export["seconds"]
    progress = [e["time"] for e in trace["events"] if e["name"] == "export_flat_progress_start"]
    snapshots = [e for e in trace["events"] if e["name"] == "export_flat_snapshot_start"]
    rows.append({"run": path.name.split(".trace")[0], "scenarios": scenarios,
                 "exportMinusBaseline": deltas, "namedMainSpans": spans, "dropped": trace["dropped"],
                 "minimumProgressIntervalMs": min((b-a)*1000 for a, b in zip(progress, progress[1:])),
                 "snapshotCount": len(snapshots), "snapshotOnMainCount": sum(e["mainThread"] for e in snapshots)})
output = evidence / "summary.json"
output.write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n")
print(output)
