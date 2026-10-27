"""Build a small redistributable smoke set; expectations require human adjudication.

Originals are CC0/public domain, not generated photos. Transformations stay in their
source split. This is a plumbing/diagnostic set, not a general retrieval benchmark.
"""

import argparse
import hashlib
import io
import json
import os
import urllib.request
from pathlib import Path

from PIL import Image, ImageDraw, ImageEnhance

REV = "e8a42ba85aaf5fd9322ef9ca51bc21063b22fcae"
SOURCE = f"https://raw.githubusercontent.com/scikit-image/scikit-image/{REV}/skimage/data/_fetchers.py"
# id, source file, license, creator, caption, query, split
SOURCES = [
    (
        "astronaut",
        "astronaut.png",
        "public-domain",
        "NASA",
        "An astronaut wearing a spacesuit.",
        "a person ready for space travel",
        "dev",
    ),
    (
        "camera",
        "camera.png",
        "CC0-1.0",
        "Lav Varshney",
        "A person operating a camera on a tripod.",
        "a photographer working outdoors",
        "dev",
    ),
    (
        "cat",
        "chelsea.png",
        "CC0-1.0",
        "Stefan van der Walt",
        "A close photograph of a tabby cat.",
        "a furry domestic feline",
        "dev",
    ),
    (
        "coffee",
        "coffee.png",
        "CC0-1.0",
        "Rachel Michetti",
        "A cup of coffee on a table.",
        "a hot caffeinated drink in a cup",
        "dev",
    ),
    (
        "rocket",
        "rocket.jpg",
        "public-domain",
        "SpaceX",
        "A rocket launching into the sky.",
        "a spacecraft lifting off at night",
        "dev",
    ),
    (
        "horse",
        "horse.png",
        "CC0-1.0",
        "Andreas Preuss",
        "A black and white silhouette of a horse.",
        "the outline of an equine animal",
        "dev",
    ),
    (
        "retina",
        "retina.jpg",
        "CC0-1.0",
        "Mikael Häggström",
        "A medical photograph of the retina of an eye.",
        "a view of the back of a human eye",
        "holdout",
    ),
    (
        "galaxies",
        "hubble_deep_field.jpg",
        "public-domain",
        "NASA",
        "Distant galaxies and stars in deep space.",
        "a telescope view of many distant galaxies",
        "holdout",
    ),
]


def sha(b):
    return hashlib.sha256(b).hexdigest()


def prepare(root, manifest_path, download):
    root.mkdir(parents=True, exist_ok=True)
    images = root / "images"
    images.mkdir(exist_ok=True)
    old = json.loads(manifest_path.read_text()) if manifest_path.exists() else None
    expected = {x["id"]: x for x in old["assets"]} if old else {}
    assets = []
    for ident, filename, license_, creator, caption, query, split in SOURCES:
        url = f"https://raw.githubusercontent.com/scikit-image/scikit-image/{REV}/skimage/data/{filename}"
        source = root / ("source-" + filename)
        if not source.exists():
            if not download:
                raise ValueError(f"{source} missing; use --download explicitly")
            if os.environ.get("INVECTUS_OFFLINE", "").lower() in ["1", "true"]:
                raise ValueError("fixture download refused: INVECTUS_OFFLINE")
            source.write_bytes(urllib.request.urlopen(url, timeout=60).read())
        raw = source.read_bytes()
        record = {
            "id": ident,
            "source": url,
            "source_sha256": sha(raw),
            "license": license_,
            "creator": creator,
            "license_evidence": SOURCE,
            "caption": caption,
            "query": query,
            "split": split,
        }
        if (
            ident in expected
            and record["source_sha256"] != expected[ident]["source_sha256"]
        ):
            raise ValueError(f"source checksum mismatch: {ident}")
        with Image.open(io.BytesIO(raw)) as im:
            image = im.convert("RGB")
            # Canonical small fixture retains aspect ratio. This does not define model preprocessing.
            image.thumbnail((640, 640), Image.Resampling.LANCZOS)
            paths = variants(images, ident, image)
        record.update(paths)
        assets.append(record)
    for color in ["red", "blue"]:
        for shape in ["circle", "square"]:
            ident = f"{color}-{shape}"
            image = Image.new("RGB", (256, 256), "white")
            draw = ImageDraw.Draw(image)
            getattr(draw, "ellipse" if shape == "circle" else "rectangle")(
                (48, 48, 208, 208), fill=color
            )
            record = {
                "id": ident,
                "source": "generated:fixtures.py/v1",
                "source_sha256": None,
                "license": "CC0-1.0",
                "creator": "inVectus fixture contributors",
                "license_evidence": "docs/multimodal/README.md",
                "caption": f"A {color} {shape} on white.",
                "query": f"a {shape} colored {color}",
                "split": "dev" if color == "red" else "holdout",
            }
            record.update(variants(images, ident, image))
            assets.append(record)
    manifest = {
        "version": 1,
        "purpose": "diagnostic smoke set; not calibration or release validation",
        "judgments": {
            "status": "proposed; not human adjudicated",
            "author": "Codex",
            "rule": "same source group is relevant; color and shape must both match for diagrams",
            "limitations": [
                "No reviewed semantic-versus-visual labels",
                "Tiny set, likely pretraining exposure",
                "Source captions and group identity are weak labels",
                "Holdout source groups never used for calibration; transformations share the source split",
            ],
        },
        "assets": assets,
    }
    # Refuse to silently redefine any existing fixture or its expectations.
    if old is not None and manifest != old:
        raise ValueError(
            "manifest changed; use a new version/path after reviewing differences"
        )
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    return manifest


def variants(images, ident, image):
    paths = {}
    candidates = {
        "document": image,
        "near_duplicate": ImageEnhance.Brightness(image).enhance(0.85),
        "image_query": image.resize(
            (max(1, image.width // 2), max(1, image.height // 2)),
            Image.Resampling.BILINEAR,
        ),
    }
    for kind, im in candidates.items():
        path = images / f"{ident}-{kind}.png"
        im.save(path, optimize=False)
        paths[kind] = {"path": f"images/{path.name}", "sha256": sha(path.read_bytes())}
    return paths


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, required=True)
    p.add_argument(
        "--manifest", type=Path, default=Path("docs/multimodal/fixtures-v1.json")
    )
    p.add_argument("--download", action="store_true")
    a = p.parse_args()
    prepare(a.root, a.manifest, a.download)
