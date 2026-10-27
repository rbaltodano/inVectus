"""Offline CPU retrieval diagnostics; never opens or modifies an inVectus project.

Checks all fixture and model bytes before loading. Results are weak-label smoke
metrics and must not select a production model or fit absence calibration.
"""

import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
import statistics
import time
from pathlib import Path

# Set before any ML imports. Inference may only use verified local paths.
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["TOKENIZERS_PARALLELISM"] = "false"
import numpy as np
from artifacts import entry, verify
from PIL import Image


def peak_rss_bytes():
    if platform.system() != "Windows":
        import resource

        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        return rss if platform.system() == "Darwin" else rss * 1024
    import ctypes
    from ctypes import wintypes

    class Counters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (n, ctypes.c_size_t)
            for n in [
                "PeakWorkingSetSize",
                "WorkingSetSize",
                "QuotaPeakPagedPoolUsage",
                "QuotaPagedPoolUsage",
                "QuotaPeakNonPagedPoolUsage",
                "QuotaNonPagedPoolUsage",
                "PagefileUsage",
                "PeakPagefileUsage",
            ]
        ]

    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    current = ctypes.windll.kernel32.GetCurrentProcess
    current.restype = wintypes.HANDLE
    get_info = ctypes.windll.psapi.GetProcessMemoryInfo
    get_info.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
    get_info.restype = wintypes.BOOL
    if not get_info(current(), ctypes.byref(counters), counters.cb):
        raise OSError("GetProcessMemoryInfo failed")
    return counters.PeakWorkingSetSize


class LegacyModel:
    def __init__(self, key, root):
        import onnxruntime as ort
        from tokenizers import Tokenizer

        self.key = key
        self.tokenizer = Tokenizer.from_file(str(root / "tokenizer.json"))
        self.tokenizer.enable_truncation(max_length=512)
        self.tokenizer.enable_padding()
        opts = ort.SessionOptions()
        opts.intra_op_num_threads = 8
        opts.inter_op_num_threads = 1
        self.session = ort.InferenceSession(
            str(root / "model.onnx"),
            sess_options=opts,
            providers=["CPUExecutionProvider"],
        )
        self.providers = self.session.get_providers()

    def encode(self, inputs, kind, query=False):
        if kind != "text":
            raise ValueError("legacy model supports text only")
        prefix = (
            (
                "Represent this sentence for searching relevant passages: "
                if query
                else ""
            )
            if self.key == "bge"
            else ("search_query: " if query else "search_document: ")
        )
        enc = self.tokenizer.encode_batch([prefix + v for v in inputs])
        values = {
            "input_ids": [e.ids for e in enc],
            "attention_mask": [e.attention_mask for e in enc],
            "token_type_ids": [e.type_ids for e in enc],
        }
        feeds = {
            i.name: np.asarray(values[i.name], dtype=np.int64)
            for i in self.session.get_inputs()
        }
        hidden = self.session.run(None, feeds)[0]
        if self.key == "bge":
            return unit(hidden[:, 0, :])
        mask = np.asarray(values["attention_mask"])[:, :, None]
        return unit((hidden * mask).sum(axis=1) / mask.sum(axis=1))


def verify_legacy(key, root):
    rows = [
        r
        for r in json.loads(
            (
                Path(__file__).resolve().parents[2] / "docs/spikes/model-manifest.json"
            ).read_text()
        )
        if r["model"] == key
    ]
    for r in rows:
        raw = (root / Path(r["file"]).name).read_bytes()
        if len(raw) != r["bytes"] or hashlib.sha256(raw).hexdigest() != r["sha256"]:
            raise ValueError("legacy artifact checksum mismatch")
    return sum(r["bytes"] for r in rows), rows[0]["revision"]


def unit(v):
    v = np.asarray(v, dtype=np.float32)
    if v.ndim != 2 or not np.isfinite(v).all():
        raise ValueError("invalid embeddings")
    norms = np.linalg.norm(v, axis=1, keepdims=True)
    if (norms <= 0).any():
        raise ValueError("zero embeddings")
    return v / norms


def rank_metrics(q, docs, qids, ids):
    scores = unit(q) @ unit(docs).T
    results = []
    for row, ident in zip(scores, qids):
        order = sorted(range(len(ids)), key=lambda i: (-float(row[i]), i))
        relevant = [i for i, x in enumerate(ids) if x == ident]
        if not relevant:
            raise ValueError(f"no relevant candidate for {ident}")
        ranks = [r + 1 for r, i in enumerate(order) if i in relevant]
        results.append(
            {
                "query": ident,
                "first_relevant_rank": min(ranks),
                "recall_at_1": sum(r <= 1 for r in ranks) / len(relevant),
                "recall_at_5": sum(r <= 5 for r in ranks) / len(relevant),
                "hit_at_1": float(min(ranks) == 1),
                "hit_at_5": float(min(ranks) <= 5),
                "reciprocal_rank": 1 / min(ranks),
                "top_5": [
                    {"id": ids[i], "similarity": float(row[i])} for i in order[:5]
                ],
                "missed_at_5": [ids[order[r - 1]] for r in ranks if r > 5],
            }
        )
    return dict(
        queries=len(results),
        **{
            k: statistics.mean(r[k] for r in results)
            for k in [
                "recall_at_1",
                "recall_at_5",
                "hit_at_1",
                "hit_at_5",
                "reciprocal_rank",
            ]
        },
        per_query=results,
    )


class TorchModel:
    def __init__(self, key, root):
        import torch

        self.torch = torch
        self.key = key
        torch.set_num_threads(8)
        if key == "gemma":
            from sentence_transformers import SentenceTransformer

            self.model = SentenceTransformer(
                str(root),
                device="cpu",
                local_files_only=True,
                config_kwargs={"audio_config": None},
                model_kwargs={"torch_dtype": torch.float32},
            )
        else:
            from transformers import AutoModel, AutoProcessor

            self.model = AutoModel.from_pretrained(
                root, local_files_only=True, torch_dtype=torch.float32
            ).eval()
            self.processor = AutoProcessor.from_pretrained(root, local_files_only=True)

    def encode(self, inputs, kind, query=False):
        with self.torch.inference_mode():
            if self.key == "gemma":
                value = [{"image": v} for v in inputs] if kind == "image" else inputs
                opts = (
                    {"prompt_name": "SearchQuery" if query else "Document"}
                    if kind == "text"
                    else {}
                )
                return unit(
                    self.model.encode(
                        value,
                        batch_size=1,
                        normalize_embeddings=True,
                        show_progress_bar=False,
                        **opts,
                    )
                )
            if kind == "image":
                values = self.processor(images=inputs, return_tensors="pt")
                features = self.model.get_image_features(**values)
            else:
                values = self.processor(
                    text=inputs,
                    padding="max_length",
                    max_length=64,
                    truncation=True,
                    return_tensors="pt",
                )
                features = self.model.get_text_features(**values)
            # Transformers 5 returns a model output in some versions.
            if hasattr(features, "pooler_output"):
                features = features.pooler_output
            return unit(features.cpu().numpy())


class OnnxModel:
    def __init__(self, root, quantized=False):
        import onnxruntime as ort
        from transformers import AutoProcessor

        opts = ort.SessionOptions()
        opts.intra_op_num_threads = 8
        opts.inter_op_num_threads = 1
        suffix = "_int8" if quantized else ""
        self.sessions = {
            kind: ort.InferenceSession(
                str(root / f"onnx/{name}_model{suffix}.onnx"),
                sess_options=opts,
                providers=["CPUExecutionProvider"],
            )
            for kind, name in [("text", "text"), ("image", "vision")]
        }
        self.processor = AutoProcessor.from_pretrained(root, local_files_only=True)
        self.providers = {k: s.get_providers() for k, s in self.sessions.items()}

    def encode(self, inputs, kind, query=False):
        if kind == "image":
            values = self.processor(images=inputs, return_tensors="np")
        else:
            values = self.processor(
                text=inputs,
                padding="max_length",
                max_length=64,
                truncation=True,
                return_tensors="np",
            )
        session = self.sessions[kind]
        feeds = {
            i.name: np.asarray(
                values[i.name],
                dtype=np.int64 if i.type == "tensor(int64)" else np.float32,
            )
            for i in session.get_inputs()
        }
        outs = session.run(None, feeds)
        names = [x.name for x in session.get_outputs()]
        desired = "image_embeds" if kind == "image" else "text_embeds"
        index = (
            names.index(desired) if desired in names else names.index("pooler_output")
        )
        return unit(outs[index])


def run(a):
    manifest_raw = a.manifest.read_bytes()
    manifest = json.loads(manifest_raw)
    for x in manifest["assets"]:
        for k in ["document", "near_duplicate", "image_query"]:
            raw = (a.fixtures / x[k]["path"]).read_bytes()
            if hashlib.sha256(raw).hexdigest() != x[k]["sha256"]:
                raise ValueError(f"fixture changed: {x['id']}/{k}")
    legacy = a.model in ["bge", "nomic"]
    if legacy:
        model_bytes, revision = verify_legacy(a.model, a.model_dir)
    else:
        model_bytes = verify(a.model, a.model_dir)
        revision = entry(a.model)["sha"]
    start = time.perf_counter()
    model = (
        LegacyModel(a.model, a.model_dir)
        if legacy
        else (
            OnnxModel(a.model_dir, a.model == "siglip-int8")
            if a.model in ["siglip-onnx", "siglip-int8"]
            else TorchModel(a.model, a.model_dir)
        )
    )
    load = time.perf_counter() - start
    # Decode the fixture images outside inference timing, in the documented RGB policy.
    assets = manifest["assets"]
    ids = [x["id"] for x in assets]
    images = [
        Image.open(a.fixtures / x["document"]["path"]).convert("RGB") for x in assets
    ]
    near = [
        Image.open(a.fixtures / x["near_duplicate"]["path"]).convert("RGB")
        for x in assets
    ]
    queries = [
        Image.open(a.fixtures / x["image_query"]["path"]).convert("RGB") for x in assets
    ]
    texts = [x["caption"] for x in assets]
    qt = [x["query"] for x in assets]
    # Warm each tower, then measure serial single-input calls (no compilation/model load).
    model.encode(texts[:1], "text")
    if not legacy:
        model.encode(images[:1], "image")
    timings = {}

    def encode(values, kind, query=False):
        t = time.perf_counter()
        out = np.concatenate([model.encode([v], kind, query) for v in values])
        timings[f"{kind}-{'query' if query else 'document'}"] = {
            "count": len(values),
            "seconds": time.perf_counter() - t,
        }
        return out

    doc_text = encode(texts, "text")
    query_text = encode(qt, "text", True)
    task_inputs = {"text_to_text": (query_text, doc_text, ids)}
    if not legacy:
        doc_image = encode(images + near, "image")
        query_image = encode(queries, "image", True)
        task_inputs.update(
            {
                "text_to_image": (query_text, doc_image, ids + ids),
                "image_to_image": (query_image, doc_image, ids + ids),
                "image_to_text": (query_image, doc_text, ids),
            }
        )
    tasks = {}
    for split in ["dev", "holdout"]:
        which = [i for i, x in enumerate(assets) if x["split"] == split]
        # All candidate groups remain available, but query groups never cross splits.
        tasks[split] = {
            task: rank_metrics(q[which], d, [ids[i] for i in which], di)
            for task, (q, d, di) in task_inputs.items()
        }
    latency = {}
    for kind, value in [("text", qt[0]), ("image", queries[0])]:
        if legacy and kind == "image":
            continue
        times = []
        for _ in range(5):
            t = time.perf_counter()
            model.encode([value], kind, True)
            times.append((time.perf_counter() - t) * 1000)
        latency[kind] = {"samples_ms": times, "median_ms": statistics.median(times)}
    rss = peak_rss_bytes()
    report = {
        "schema_version": 1,
        "model": a.model,
        "revision": revision,
        "runtime": "onnxruntime CPU INT8"
        if a.model == "siglip-int8"
        else (
            "onnxruntime CPU FP32"
            if legacy or a.model == "siglip-onnx"
            else "PyTorch CPU FP32"
        ),
        "status": "diagnostic only; proposed labels, not human reviewed; no calibration",
        "fixture_sha256": hashlib.sha256(manifest_raw).hexdigest(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "threads": 8,
        "packages": {
            p: importlib.metadata.version(p)
            for p in [
                "torch",
                "transformers",
                "sentence-transformers",
                "pillow",
                "numpy",
                "onnxruntime",
            ]
        },
        "model_bytes": model_bytes,
        "load_seconds": load,
        "peak_rss_bytes": rss,
        "timings": timings,
        "query_latency": latency,
        "tasks": tasks,
        "preprocessing": (
            "legacy pinned tokenizers; 512 head truncation; BGE CLS + query instruction, Nomic mean + search_query/search_document; L2"
            if legacy
            else "Pillow RGB; pinned AutoProcessor; SigLIP fixed 224 bilinear, rescale and normalize 0.5, text 64 max padding; Gemma pinned processor with default 280 vision tokens, SearchQuery/Document text prompts; L2"
        ),
        "providers": getattr(model, "providers", None),
    }
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {k: v for k, v in report.items() if k not in ["tasks", "packages"]},
            indent=2,
        )
    )
    for split, tasks_ in tasks.items():
        for task, m in tasks_.items():
            print(split, task, "hit@1", m["hit_at_1"], "recall@5", m["recall_at_5"])


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--model",
        choices=["gemma", "siglip", "siglip-onnx", "siglip-int8", "bge", "nomic"],
        required=True,
    )
    p.add_argument("--model-dir", type=Path, required=True)
    p.add_argument("--fixtures", type=Path, required=True)
    p.add_argument(
        "--manifest", type=Path, default=Path("docs/multimodal/fixtures-v1.json")
    )
    p.add_argument("--out", type=Path, required=True)
    run(p.parse_args())
