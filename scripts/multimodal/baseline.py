"""Read-only metadata guard for the existing benchmarks and installed text models."""

import argparse
import hashlib
import json
from pathlib import Path


def fingerprint(root):
    if not root.exists():
        return {"exists": False}
    rows = []
    for path in sorted(root.rglob("*")):
        if path.is_file():
            s = path.stat()
            rows.append([str(path.relative_to(root)), s.st_size, s.st_mtime_ns])
    return {
        "exists": True,
        "files": len(rows),
        "bytes": sum(r[1] for r in rows),
        "path_size_mtime_sha256": hashlib.sha256(
            json.dumps(rows, separators=(",", ":")).encode()
        ).hexdigest(),
    }


def snapshot():
    roots = {
        "benchmark-projects": Path(".spike-data/p1/work"),
        "benchmark-results": Path(".spike-data/p1"),
        "text-models": Path(".spike-data/models"),
        "text-model-store": Path(".spike-data/models-store"),
        "reference-vectors": Path.home()
        / "Documents/Medical Paper Vector Database/data/vectors",
    }
    data = {key: fingerprint(root) for key, root in roots.items()}
    db = Path.home() / "Documents/Medical Paper Vector Database/data/gapmap.duckdb"
    if db.exists():
        stat = db.stat()
        data["reference-database"] = {
            "bytes": stat.st_size,
            "mtime_ns": stat.st_mtime_ns,
        }
    return data


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=["capture", "check"])
    p.add_argument("path", type=Path)
    a = p.parse_args()
    data = snapshot()
    if a.command == "capture":
        # Never replace the original guard.
        with a.path.open("x") as f:
            json.dump(data, f, indent=2)
            f.write("\n")
    else:
        if json.loads(a.path.read_text()) != data:
            raise SystemExit("metadata changed; investigate before proceeding")
        print("all reference, benchmark and text-model metadata unchanged")
