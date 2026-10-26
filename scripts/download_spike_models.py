"""Download only the two permitted embedding models, pinned by HF revision."""
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen

MODELS = {
    "nomic": ("nomic-ai/nomic-embed-text-v1.5", "e9b6763023c676ca8431644204f50c2b100d9aab", 547310275),
    "bge": ("Xenova/bge-small-en-v1.5", "ea104dacec62c0de699686887e3f920caeb4f3e3", 133093490),
}
assert sum(m[2] for m in MODELS.values()) < 1_000_000_000
manifest_path = Path('docs/spikes/model-manifest.json')
expected = {(m['model'],m['file']): m['sha256'] for m in json.loads(manifest_path.read_text())} if manifest_path.exists() else {}
manifest = []
for name, (repo, revision, size) in MODELS.items():
    root = Path('.spike-data/models') / name
    root.mkdir(parents=True, exist_ok=True)
    for filename in ['onnx/model.onnx', 'tokenizer.json', 'config.json', 'special_tokens_map.json', 'tokenizer_config.json']:
        dest = root / Path(filename).name
        if not dest.exists():
            with urlopen(f'https://huggingface.co/{repo}/resolve/{revision}/{filename}') as src, dest.with_suffix('.tmp').open('wb') as out:
                while chunk := src.read(1024 * 1024):
                    out.write(chunk)
            dest.with_suffix('.tmp').replace(dest)
        if filename.endswith('.onnx'):
            assert dest.stat().st_size == size
        digest = hashlib.sha256(dest.read_bytes()).hexdigest()
        if (name,filename) in expected:
            assert digest == expected[name,filename], f'checksum mismatch: {dest}'
        manifest.append(dict(model=name, repo=repo, revision=revision, file=filename, bytes=dest.stat().st_size, sha256=digest))
        print(name, filename, dest.stat().st_size, flush=True)
Path('docs/spikes/model-manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
