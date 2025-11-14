#!/usr/bin/env python3
"""
Merge persisted test JSON reports into a single JSON array.

Usage: python scripts/merge_test_reports.py [--out merged.json]
Searches all `**/target/test-generated/*.json` and merges them
by reading each JSON file as an object and appending it to a JSON array.
"""
import json
from pathlib import Path
import argparse

parser = argparse.ArgumentParser()
parser.add_argument("--out", default="merged-test-reports.json")
args = parser.parse_args()

root = Path(".")
files = list(root.glob("**/target/test-generated/*.json"))
reports = []
for f in files:
    try:
        j = json.loads(f.read_text())
        reports.append({"path": str(f), "report": j})
    except Exception as e:
        print(f"Skipping {f}: {e}")

out_path = Path(args.out)
out_path.write_text(json.dumps(reports, indent=2))
print(f"Wrote {out_path} containing {len(reports)} reports")
