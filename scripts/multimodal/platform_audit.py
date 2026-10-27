"""Record actual PyPI wheel availability; availability does not prove execution."""

import json
import urllib.request
from pathlib import Path

versions = {
    "torch": "2.14.1",
    "transformers": "5.21.0",
    "sentence-transformers": "6.2.0",
    "onnxruntime": "1.31.1",
    "pillow": "13.0.0",
    "torchvision": "0.29.1",
    "soundfile": "0.14.0",
}
platforms = {
    "macos-arm64": ["macosx", "arm64"],
    "windows-x86_64": ["win_amd64"],
    "linux-x86_64": ["manylinux", "x86_64"],
}
report = {}
for package, version in versions.items():
    with urllib.request.urlopen(
        f"https://pypi.org/pypi/{package}/{version}/json", timeout=30
    ) as r:
        data = json.load(r)
    wheels = [
        f
        for f in data["urls"]
        if f["filename"].endswith(".whl")
        and (
            "cp312" in f["filename"]
            or ("py3-none" in f["filename"] or "py2.py3-none" in f["filename"])
        )
    ]
    report[package] = {
        "version": version,
        "requires_python": data["info"]["requires_python"],
        "platforms": {},
    }
    for platform, terms in platforms.items():
        report[package]["platforms"][platform] = [
            {
                "filename": f["filename"],
                "sha256": f["digests"]["sha256"],
                "bytes": f["size"],
            }
            for f in wheels
            if all(t in f["filename"] for t in terms)
            or ("py3-none-any" in f["filename"] or "py2.py3-none-any" in f["filename"])
        ]
Path("docs/multimodal/platform-artifacts.json").write_text(
    json.dumps(
        {
            "status": "wheel metadata only; macOS arm64 executed separately; Windows/Linux unexecuted",
            "python": "CPython 3.12",
            "packages": report,
        },
        indent=2,
    )
    + "\n"
)
