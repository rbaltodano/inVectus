#!/usr/bin/env python3
"""Offline publication check of reachable Git blobs and current non-ignored files.

Reports paths and detection types only, never matched credential values. This is a
heuristic check, not a comprehensive secret scanner or a guarantee of safe publication.
Usage: python3 scripts/audit_publication.py > .spike-data/publication-audit.json
"""
import json
from pathlib import Path
import re
import subprocess

RULES = {
    "private-key": re.compile(rb"-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----"),
    "github-token": re.compile(rb"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{60,255})\b"),
    "aws-access-key": re.compile(rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    "huggingface-token": re.compile(rb"\bhf_[A-Za-z0-9]{30,100}\b"),
    "openai-or-anthropic-token": re.compile(rb"\bsk-(?:proj-|ant-api\d+-)?[A-Za-z0-9_-]{32,255}\b"),
    "slack-token": re.compile(rb"\bxox[baprs]-[A-Za-z0-9-]{20,255}\b"),
    "google-api-key": re.compile(rb"\bAIza[0-9A-Za-z_-]{35}\b"),
    "literal-credential-assignment": re.compile(rb'''(?i)["']?(?:api[_-]?key|client[_-]?secret|access[_-]?token|password)["']?\s*[:=]\s*["']([^\s"']{12,})["']'''),
}
SUSPICIOUS = re.compile(r"(?:^|/)(?:\.env(?:\.[^/]*)?|id_rsa|id_ed25519|credentials\.json)(?:$|/)|\.(?:pem|p12|pfx|duckdb|onnx|gguf|safetensors|pt|pth|ckpt|parquet|npy|npz|pkl)$|\.invectus/|\.lance/", re.I)
MAX_BYTES = 2_000_000


def git(*args):
    return subprocess.check_output(["git", *args])


def inspect(data):
    matches = []
    for name, pattern in RULES.items():
        for match in pattern.finditer(data):
            if name == "literal-credential-assignment":
                value = match.group(1).lower()
                if any(word in value for word in [b"example", b"placeholder", b"your_", b"your-", b"test", b"dummy", b"${", b"process.env", b"os.environ"]):
                    continue
            matches.append({"rule": name, "line": data[:match.start()].count(b"\n") + 1})
    return matches


def main():
    objects = {}
    for line in git("rev-list", "--objects", "--all").decode().splitlines():
        oid, _, path = line.partition(" ")
        objects.setdefault(oid, path)
    checks = subprocess.check_output(
        ["git", "cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"],
        input=("\n".join(objects) + "\n").encode())
    blobs, skipped_large = [], []
    for line in checks.decode().splitlines():
        oid, kind, size = line.split()
        if kind == "blob":
            if int(size) <= MAX_BYTES:
                blobs.append(oid)
            else:
                skipped_large.append({"object": oid, "path": objects[oid], "bytes": int(size)})
    output = subprocess.check_output(["git", "cat-file", "--batch"], input=("\n".join(blobs) + "\n").encode())
    offset, text_count, binary_count, findings = 0, 0, 0, []
    for oid in blobs:
        end = output.index(b"\n", offset)
        size = int(output[offset:end].split()[-1])
        data = output[end + 1:end + 1 + size]
        offset = end + size + 2
        if b"\0" in data:
            binary_count += 1
            continue
        text_count += 1
        for finding in inspect(data):
            findings.append(dict(scope="reachable-history", object=oid, path=objects[oid], **finding))
    paths = set(git("ls-files", "-z").decode().split("\0")) | set(git("ls-files", "--others", "--exclude-standard", "-z").decode().split("\0"))
    worktree_skipped = []
    for name in sorted(paths - {""}):
        path = Path(name)
        if not path.is_file():
            continue
        if path.stat().st_size > MAX_BYTES:
            worktree_skipped.append(name)
            continue
        data = path.read_bytes()
        if b"\0" not in data:
            findings.extend(dict(scope="worktree", path=name, **f) for f in inspect(data))
    history_paths = set(objects.values())
    report = {
        "scope": "All local Git refs reachable through --all, plus tracked and non-ignored untracked worktree files; no network or uploads",
        "limitation": "Pattern-based heuristic; does not prove absence of secrets, inspect binary content, scan unreachable objects, or audit third-party rights",
        "commits": int(git("rev-list", "--all", "--count")),
        "history_text_blobs_scanned": text_count, "history_binary_blobs_skipped": binary_count,
        "history_large_blobs_skipped": skipped_large, "worktree_large_files_skipped": worktree_skipped,
        "suspicious_artifact_paths": sorted(p for p in (paths | history_paths) if SUSPICIOUS.search(p) and not p.endswith(".env.example")),
        "credential_candidates": findings,
    }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
