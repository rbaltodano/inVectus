"""Prepare verified SigLIP smoke inputs for the Rust ort example; no inference downloads."""

import argparse
import hashlib
import json
from pathlib import Path

from artifacts import verify
from evaluate import OnnxModel
from PIL import Image

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--model-dir", type=Path, required=True)
p.add_argument("--fixtures", type=Path, required=True)
p.add_argument(
    "--manifest", type=Path, default=Path("docs/multimodal/fixtures-v1.json")
)
p.add_argument("--out", type=Path, required=True)
a = p.parse_args()
verify("siglip-onnx", a.model_dir)
asset = json.loads(a.manifest.read_text())["assets"][0]
path = a.fixtures / asset["document"]["path"]
if hashlib.sha256(path.read_bytes()).hexdigest() != asset["document"]["sha256"]:
    raise ValueError("fixture changed")
model = OnnxModel(a.model_dir)
image = Image.open(path).convert("RGB")
pixel_values = model.processor(images=[image], return_tensors="np")["pixel_values"]
input_ids = model.processor(
    text=[asset["query"]],
    padding="max_length",
    max_length=64,
    truncation=True,
    return_tensors="np",
)["input_ids"]
reference = {
    kind: model.encode([value], kind, True)[0].tolist()
    for kind, value in [("text", asset["query"]), ("image", image)]
}
a.out.write_text(
    json.dumps(
        {
            "source_id": asset["id"],
            "pixel_values": pixel_values.reshape(-1).tolist(),
            "input_ids": input_ids.reshape(-1).tolist(),
            "reference": reference,
        }
    )
    + "\n"
)
