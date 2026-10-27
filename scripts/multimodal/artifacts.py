"""Explicit pinned spike installation. Never called implicitly by inference."""

import argparse
import hashlib
import json
import os
import urllib.request
from pathlib import Path

AUDIT = Path(__file__).resolve().parents[2] / "docs/multimodal/artifact-audit.json"
KEYS = {
    "gemma": "google/embeddinggemma-2",
    "siglip": "google/siglip2-base-patch16-224",
    "siglip-onnx": "onnx-community/siglip2-base-patch16-224-ONNX",
    "siglip-int8": "onnx-community/siglip2-base-patch16-224-ONNX",
}


def entry(key):
    return next(x for x in json.loads(AUDIT.read_text()) if x["id"] == KEYS[key])


def files(key):
    d = entry(key)
    for f in d["siblings"]:
        name = f["rfilename"]
        if name.endswith(
            (".json", ".model", ".jinja", ".safetensors")
        ) and not name.startswith("onnx/"):
            yield f
        if key == "siglip-onnx" and name in [
            "onnx/text_model.onnx",
            "onnx/vision_model.onnx",
        ]:
            yield f
        if key == "siglip-int8" and name in [
            "onnx/text_model_int8.onnx",
            "onnx/vision_model_int8.onnx",
        ]:
            yield f


def verify(key, root):
    for f in files(key):
        path = root / f["rfilename"]
        raw = path.read_bytes()
        if len(raw) != f["size"]:
            raise ValueError(f"{path}: wrong size")
        if "lfs" in f:
            got = hashlib.sha256(raw).hexdigest()
            expected = f["lfs"]["sha256"]
        else:
            got = hashlib.sha1(
                b"blob " + str(len(raw)).encode() + b"\0" + raw
            ).hexdigest()
            expected = f["blobId"]
        if got != expected:
            raise ValueError(f"{path}: checksum mismatch")
    return sum(f["size"] for f in files(key))


def download(key, root):
    if os.environ.get("INVECTUS_OFFLINE", "").lower() in ["1", "true"]:
        raise ValueError("explicit download refused: INVECTUS_OFFLINE")
    d = entry(key)
    # HTTPS at a pinned revision, no code execution or remote custom model code.
    for f in files(key):
        target = root / f["rfilename"]
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            continue
        url = f"https://huggingface.co/{d['id']}/resolve/{d['sha']}/{f['rfilename']}"
        partial = target.with_suffix(target.suffix + ".partial")
        print("download", f["rfilename"], flush=True)
        with (
            urllib.request.urlopen(url, timeout=120) as response,
            partial.open("wb") as dest,
        ):
            while chunk := response.read(1024 * 1024):
                dest.write(chunk)
        partial.replace(target)
    verify(key, root)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=["download", "verify"])
    p.add_argument("--model", required=True, choices=KEYS)
    p.add_argument("--root", required=True, type=Path)
    a = p.parse_args()
    if a.command == "download":
        download(a.model, a.root)
    print(f"verified {verify(a.model, a.root)} bytes")
