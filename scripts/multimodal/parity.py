"""Measure the exported/quantized towers against the pinned PyTorch SigLIP 2.

This does not grant parity to any inVectus project, nor calibrate a verdict.
"""

import argparse
import hashlib
import json
import platform
from pathlib import Path

import numpy as np
from artifacts import entry, verify
from evaluate import OnnxModel, TorchModel, unit
from PIL import Image

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--torch-dir", type=Path, required=True)
p.add_argument("--onnx-dir", type=Path, required=True)
p.add_argument("--fixtures", type=Path, required=True)
p.add_argument(
    "--manifest", type=Path, default=Path("docs/multimodal/fixtures-v1.json")
)
p.add_argument("--out", type=Path, required=True)
p.add_argument("--int8", action="store_true")
a = p.parse_args()
key = "siglip-int8" if a.int8 else "siglip-onnx"
verify("siglip", a.torch_dir)
verify(key, a.onnx_dir)
raw = a.manifest.read_bytes()
assets = json.loads(raw)["assets"]
images = []
for x in assets:
    path = a.fixtures / x["document"]["path"]
    if hashlib.sha256(path.read_bytes()).hexdigest() != x["document"]["sha256"]:
        raise ValueError("fixture changed")
    images.append(Image.open(path).convert("RGB"))
texts = [x["query"] for x in assets]
torch = TorchModel("siglip", a.torch_dir)
onnx = OnnxModel(a.onnx_dir, a.int8)
report = {
    "reference_revision": entry("siglip")["sha"],
    "candidate_revision": entry(key)["sha"],
    "candidate": key,
    "platform": platform.platform(),
    "machine": platform.machine(),
    "fixture_sha256": hashlib.sha256(raw).hexdigest(),
    "status": "diagnostic parity only; no production table or calibration modified",
    "modalities": {},
}
outputs = {}
for kind, values in [("text", texts), ("image", images)]:
    reference = np.concatenate([torch.encode([v], kind, True) for v in values])
    candidate = np.concatenate([onnx.encode([v], kind, True) for v in values])
    cosine = (unit(reference) * unit(candidate)).sum(axis=1)
    report["modalities"][kind] = {
        "sample": len(values),
        "min_cosine": float(cosine.min()),
        "mean_cosine": float(cosine.mean()),
        "max_absolute_error": float(np.abs(reference - candidate).max()),
        "per_source_cosine": dict(zip([x["id"] for x in assets], map(float, cosine))),
    }
    outputs[kind] = (reference, candidate)
r = outputs["text"][0] @ outputs["image"][0].T
c = outputs["text"][1] @ outputs["image"][1].T
report["text_to_image_top1_agreement"] = float(
    (r.argmax(axis=1) == c.argmax(axis=1)).mean()
)
report["max_cross_modal_similarity_delta"] = float(np.abs(r - c).max())
a.out.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
