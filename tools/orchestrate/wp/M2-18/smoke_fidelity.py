#!/usr/bin/env python3
"""Local integration smoke test; references are synthetic, NOT Lightroom evidence."""
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent
BIN = Path(os.environ["CARGO_TARGET_DIR"]) / "release/tessera"
CAT = OUT / "synthetic-smoke.lrcat"
REF = OUT / "synthetic-references"
REF.mkdir(exist_ok=True)
# Reuse the repository's documented minimal catalog schema, not a real catalog.
match = re.search(r'execute_batch\(r#"(.*?)"#\)',
                   (ROOT / "crates/import-lrcat/tests/make_fixture.rs").read_text(),
                   re.S)
assert match is not None, "fixture schema was not found"
schema = match.group(1)
if CAT.exists():
    CAT.unlink()
with sqlite3.connect(CAT) as db:
    db.executescript(schema)
    db.execute("UPDATE AgLibraryRootFolder SET absolutePath=?",
               (str(ROOT / "fixtures/raw") + "/",))
    db.execute("UPDATE AgLibraryFolder SET pathFromRoot='' WHERE id_local=10")
    db.execute("UPDATE AgLibraryFile SET baseName='sample',extension='dng' WHERE id_local=20")
    db.execute("INSERT INTO Adobe_imageDevelopSettings VALUES(30,?, '15.4')",
               ('<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" '
                'xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4"/>',))
subprocess.run(["sips", "-s", "format", "jpeg", "-s", "formatOptions", "100",
                str(OUT / "adobe-smoke.png"), "--out", str(REF / "30.jpg")], check=True)
result = subprocess.run([str(BIN), "--app-dir", str(OUT / "smoke-app"), "--json",
                         "import", "lrcat", str(CAT), "--fidelity", "--reference-dir", str(REF)],
                        check=True, capture_output=True, text=True)
report = json.loads(result.stdout)
report["reference_provenance"] = "synthetic: compatibility render encoded to JPEG, NOT Lightroom"
fidelity = report["fidelity"]
assert fidelity["compared"] == 1, report
row = next(row for row in fidelity["images"] if row["catalog_id"] == 30)
assert row["status"] == "compared" and row["samples"] > 0, row
assert row["mean"] < 1.0 and row["p95"] < 2.0, row
(OUT / "fidelity-smoke.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({"reference": report["reference_provenance"], "compared":fidelity["compared"],
                  "mean":row["mean"], "p95":row["p95"], "samples":row["samples"]}, indent=2))
