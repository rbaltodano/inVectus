# ort / fastembed CPU embeddings

Measured candidate: direct `ort` 2.0.0-rc.15 / ONNX Runtime CPU, `AutoDevicePolicy::PreferCPU`, explicit 8 intra-op threads, batch 32, max 512 tokens. [`fastembed` 7.5.0](https://docs.rs/fastembed/7.5.0/fastembed/) was compiled and preflighted but has the provider-policy limitation below. No CoreML, MPS, CUDA or DirectML provider was enabled.

Models: `nomic-ai/nomic-embed-text-v1.5` (768-D FP32) versus `Xenova/bge-small-en-v1.5` (384-D FP32 ONNX export of BAAI BGE). [model-manifest.json](model-manifest.json) pins repository commits and SHA-256 for every downloaded file. The two weights total **680,403,765 bytes**, plus about 1.43 MB tokenizer/config files. No LLM or additional embedding weights were downloaded.

Method: same 256 real title+abstract strings copied from the reference, mean 897 characters, median 800, maximum 2,701. Nomic gets `search_document: `; BGE documents have no prefix. Mean pooling + full-dimensional centering/L2 normalization for Nomic; CLS + L2 for BGE. Both have a common 512-token cap for comparison (Nomic's longer model context was not exercised). The [Nomic card](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5) and [BGE card](https://huggingface.co/BAAI/bge-small-en-v1.5) define their respective prefixes/pooling conventions.

Load a pinned local model (no Hub calls during inference), warm up 8 items, then time three 256-item passes, including tokenization, pooling and normalization. Validate finite unit-length output vectors. Model load (including reading the local ONNX file and graph optimization) is reported separately. Retrieval semantic quality and ONNX parity with gapmap are **not** established by this speed test. Existing gapmap vectors stay in their original model space.

## Results and verdict

| CPU model | Three passes (items/s) | Median | Session load | Mean capped tokens |
|---|---|---:|---:|---:|
| Nomic v1.5 | 26.69, 26.47, 27.31 | **26.69/s** | 0.242 s | 207.64 |
| BGE-small v1.5 | 71.18, 72.33, 71.95 | **71.95/s** | 0.085 s | 203.71 |

Five of 256 texts reached the 512-token cap for each tokenizer. All output vectors passed finite/unit-norm checks. Raw timings: `nomic-results.json`, `bge-results.json`.

**Go for direct ort CPU inference. BGE-small is the recommended provisional CPU default/performance preset; Nomic is no-go for the 60 items/s CPU target in this configuration.** BGE was 2.70× faster here. Embedding-only extrapolation is ~23 minutes/100k for BGE versus ~62 minutes for Nomic, before import/index overhead; neither is a 100k validation. Retain Nomic as the reference quality candidate pending retrieval tests, model-output parity and calibration. Speed alone does not establish a quality winner, and the 512-token cap must be explicit in the import policy.

## Provider-policy finding

`ort` rc.15 defaults sessions to `AutoDevicePolicy::MaxEfficiency`, documented to automatically select CoreML on macOS. Passing an explicit CPU execution provider is insufficient to disable automatic selection. Fastembed 7.5.0's public options do not expose `with_auto_device`; its empty-provider and explicit-CPU preflights both began CoreML compilation and were stopped. **No-go for an unmodified fastembed CPU baseline** until it exposes the policy or equivalent session-builder control. This is not a claim that accelerated fastembed cannot work.

The measured executable instead uses `Session::builder(...).with_auto_device(PreferCPU)` with the same pinned ONNX files and Rust tokenization/pooling. CPU execution was observed using ~6–7 cores with no CoreML model files open. Stopped accelerator preflight results are excluded. This finding reinforces why P1 must record the actual execution provider, not assume an empty list means CPU.
