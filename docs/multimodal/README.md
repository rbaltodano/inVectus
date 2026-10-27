# Multimodal evaluation and implementation checkpoint — 2026-10-27

This is a **diagnostic spike and additive interface foundation**, not completed image-search
support. The model/runtime choice and image application integration remain gated on reviewed
relevance judgments and native platform execution. BGE remains the default; Nomic remains
available. No production collection, index, calibration, or benchmark vector was changed.

## Baseline and scope

- Starting revision: `b5f6d71fda3d316ed4391cd68bc7be9225bf541a`. The only initial working-tree
  change was the owner's untracked `docs/multimodal-implementation-plan.md`. No repository-local
  `AGENTS.md` or `CLAUDE.md` exists; the parent instructions apply their model restrictions only
  to Aquinas projects. No running benchmark/ablation/Cargo processes were found. Existing idle
  Python HTTP servers were left alone.
- Read `PROGRESS.md`, `SPEC.md`, the plan, embedding/identity/store/index/search code, registry,
  desktop commands/UI, evaluation code and CI. The full 591,484-paper benchmark and term
  ablation are finished and recorded, with raw output in `.spike-data/p1/`. The model still
  loses to raw demand count and density. No weights were tuned; no full benchmark was rerun or
  re-embedded. The unresolved meaning of "filled" is independent of media embeddings.
- Before code changes, `cargo test --workspace --locked` passed **75 tests**, including the
  installed BGE model and CLI integration tests. The older ledger's 74 count predates the
  latest ablation/scaling changes. Logs are in ignored `.spike-data/multimodal-baseline-tests.log`.
- `baseline-data-guard.json` records file-count/size/path/mtime fingerprints of the existing
  benchmark projects, results, text models and reference vectors, plus reference database
  size/mtime. End-of-task checks must match. This detects metadata changes; it is not a new
  claim of byte-for-byte reference integrity and does not resolve the older ledger's unexplained
  fingerprint discrepancy.
- Test-only golden identities were read using **read-only DuckDB** from the small demo and
  imported gapmap project. They contain model policy only, not source text. Their exact BGE,
  Nomic and foreign-runtime Nomic space IDs are tested without modifying those projects.

## Evaluation set and the data gate

`fixtures-v1.json` pins 12 source groups: 8 CC0/public-domain images and 4 original color/shape
cards. Each has a document image, brightness-adjusted near duplicate, and half-resolution query.
The source URLs use scikit-image commit `e8a42ba85aaf5fd9322ef9ca51bc21063b22fcae`; individual
license evidence is linked in the manifest. The NASA/SpaceX originals are public domain;
the other selected originals are explicitly CC0. The newly authored diagrams and fixture
expectations are dedicated to CC0-1.0 for redistribution. No other repository code is relicensed.

The 8 development groups and 4 holdout groups are disjoint by source. Derivatives stay with
their source; all 12 groups remain candidate distractors. Candidate counts: 12 text documents,
24 images. Tasks: text→text, text→image, image→image and image→text. The evaluator reports
hit@1/5, actual recall@1/5, reciprocal rank, ranked scores and missed relevant targets. The two
image variants count as **two** relevant targets, so perfect image hit@1 has recall@1 = 0.5.

**These are proposed agent-authored expectations, not human-adjudicated judgments.** Source
captions and source identity provide weak labels. Every measured configuration has hit@1,
recall@5 and MRR = 1.0 for every applicable task in both splits. This is useful evidence that
the towers and prompts work, but the set is too easy, too small and likely exposed during
pretraining. It cannot distinguish semantic versus visual retrieval quality, estimate missed
matches in realistic collections, select a default, or train/calibrate any verdict. The
holdout is a reserved source split, not a validated calibration set; it has only been used for
diagnostic reporting, with no fitting/tuning.

**Next data gate:** independent human judgments on realistic queries, visually confusable
items and semantic/visual divergence; several relevant assets per query; unrelated distractors;
source-group-separated development, retrieval test and untouched calibration partitions.
Record the adjudicator and disagreements. Do not silently relabel this frozen manifest.

Existing human-judged datasets were also checked (see `dataset-access-audit.json`). Official
[Winoground](https://huggingface.co/datasets/facebook/winoground) at `b400e173…` is access-gated,
requires agreement to research-only terms, and its parquet HEAD request returned **HTTP 401**
without credentials. No agreement was accepted or credentials accessed. The ungated
[MMEB evaluation collection](https://huggingface.co/datasets/TIGER-Lab/MMEB-eval) at `2f069730…`
is an alternative for subsequent evaluation: its card declares Apache-2.0 and identifies the
first target as ground truth, but image tasks reference a separate **7,128,863,486-byte** ZIP.
No blanket claim was made that this label overrides original constituent-image terms.
It was not bulk-downloaded onto this low-headroom host; a next run can select and verify a
bounded per-task subset and its source terms. These checks establish a concrete access/storage
limitation for those routes, not that public human-labeled evaluation is impossible.

## Verified artifacts, licenses and runtime integration

`artifact-audit.json` records HF repository revisions, actual file listings/sizes, LFS SHA-256s,
Git blob IDs for small files, exact configs and model-card checksums/links. No repository code
is loaded with `trust_remote_code`. Installation is explicit; inference uses verified local
files, `local_files_only`, `HF_HUB_OFFLINE` and `TRANSFORMERS_OFFLINE`.

| Candidate | Verified revision | Artifacts and terms | Local integration evidence |
|---|---|---|---|
| Google EmbeddingGemma 2 | `914f7f89142e33e77833254d9c9b90c3cef7303b` | Published 1,488,915,288-byte safetensors checkpoint and processor/tokenizer files; model card declares Apache-2.0; ungated | PyTorch 2.14.1 + Transformers 5.21.0 + SentenceTransformers 6.2.0, CPU FP32, audio encoder disabled; actual offline text/image retrieval on macOS arm64. **No ONNX file in this pinned repository**, so not a drop-in for the current Rust encoder. No native packaged adapter tested. |
| Google SigLIP 2 base patch16 224 | `75de2d55ec2d0b4efc50b3e9ad70dba96a7b2fa2` | Published 1,500,800,904-byte safetensors checkpoint; Apache-2.0 card; ungated | PyTorch CPU FP32 text and image towers executed offline |
| ONNX-community export of that SigLIP 2 | `ba1f3b0843f24bc5417d38e19c37b287d719b2f4` | Separate FP32 and INT8 text/vision ONNX graphs verified against actual bytes. Export card names the Google base but **omits its own license field and LICENSE file** | Python ORT 1.31.1 with explicit CPUExecutionProvider executed both precisions. Native Rust `ort` preflight supplied separately. Distribution packaging must preserve upstream Apache license/notice and export provenance; no independent export license grant was inferred. |

The cards/configs are evidence of advertised capabilities, not tested media coverage:

- Gemma's pinned configuration supports a joint 768-D space, with mean pooling and normalization.
  The spike used `SearchQuery` for text queries, `Document` for untitled captions and no prompt
  for images. It uses the pinned processor's 280 vision-token default. Current docs prescribe
  FP32 or BF16; **do not use FP16**. BF16/accelerators were not measured. The processor needed
  torchvision even with audio disabled; installing just the packages in the quickstart was
  insufficient in the isolated environment. Added torchvision 0.29.1 and soundfile 0.14.0.
- Do not infer limits from architecture capacity: the model card says 8,192 input tokens, while
  `text_config.max_position_embeddings` is 262,144. The card and config also disagree about
  sliding-window size (1,024 vs 512). The spike used only short inputs and does not resolve
  those differences. Video config says 1 fps, max 32 frames, 140 soft tokens/frame, uniform
  overflow. Audio config is 16 kHz. **Neither video nor audio was executed or evaluated.**
- SigLIP uses a fixed 224×224 bilinear resize, RGB, rescale 1/255 and channel mean/std 0.5, and
  64-token padded text. Projected pooled tower outputs are L2-normalized. The separate ONNX
  towers expose `pooler_output`; they cannot use the existing text encoder's rank-3 hidden-state
  pooling path. The spike's image conversion assumes the fixtures are RGB; EXIF, transparency,
  safe decoding, and thumbnail policy are still production import work, not proven here.

Primary references: [EmbeddingGemma 2 card](https://ai.google.dev/gemma/docs/embeddinggemma/model_card_2),
[Google multimodal guide](https://ai.google.dev/gemma/docs/embeddinggemma/multimodal-embeddinggemma-with-sentence-transformers),
[SigLIP 2 card](https://huggingface.co/google/siglip2-base-patch16-224),
[ONNX export card](https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX),
[ONNX Runtime compatibility](https://onnxruntime.ai/docs/reference/compatibility.html).

## Measurements (diagnostic, not product targets)

M4 Pro, 24 GiB RAM, macOS 27.0.1 arm64; 8 CPU intra-op threads, batch 1. Models ran serially
without a competing benchmark/compiler; separate artifact downloads could overlap disk/network
I/O. Fixtures were decoded before timing; preprocessing, tokenization and inference are included.
Warm query latency is the median of 5 repetitions of the first source, not a workload p95.
Image throughput is 24 serial document encodes. Peak RSS includes Python/frameworks, full-file checksum verification and model
loading, not just resident weights; disk is the set of verified inference artifacts, not Python
packages. The isolated Python environment adds about 1 GB on this Mac. Load time includes
model-class imports and differs by backend. These short-caption timings do not replace the
existing long-text benchmark results.

| Model/runtime | Artifacts MB | Peak process RSS MB | Load s | Text query ms | Image query ms | Images/s |
|---|---:|---:|---:|---:|---:|---:|
| Existing BGE / ORT FP32 | 133.8 | 429.8 | 0.82 | 1.98 | — | — |
| Existing Nomic / ORT FP32 | 548.0 | 1369.1 | 0.21 | 4.90 | — | — |
| EmbeddingGemma 2 / PyTorch FP32 | 1525.8 | 4571.7 | 4.16 | 27.61 | 694.85 | 1.44 |
| SigLIP 2 / PyTorch FP32 | 1539.5 | 2492.2 | 2.54 | 17.17 | 32.82 | 30.43 |
| SigLIP 2 / ORT FP32 | 1539.9 | 4536.5 | 2.61 | 8.16 | 33.19 | 34.40 |
| SigLIP 2 / ORT INT8 | 416.6 | 1910.6 | 2.40 | 7.28 | 19.60 | 49.53 |

Raw per-query results are `*-cpu.json`. BGE and Nomic used the pinned existing models, tokenizer,
query/document prefixes and pooling in an isolated Python diagnostic, not their production
Rust timing harness; no existing vectors were reused or mixed. Only 12 tiny text documents
were encoded per candidate. Existing text quality/calibration findings in `PROGRESS.md` remain
more informative than these ceiling-level smoke results.

`siglip-onnx-parity.json`: FP32 ONNX versus PyTorch on 12 original images and 12 queries, minimum
cosine 1.000000 at report precision for both towers, maximum cross-modal score delta 3.19e-7,
text→image top-1 agreement 1.0. This is a **same-Mac** export check, not cross-platform parity.
`siglip-int8-parity.json`: minimum cosine **0.99556 text / 0.96141 image**, maximum cross-modal
score delta **0.01406**. Top-1 remained unchanged on these easy cases. INT8 is a distinct space;
it is not approved for FP32 table reuse or calibration reuse. Gemma quantization is untested.

## Platform evidence and decision

| Target | Artifact availability | Actual execution | Product packaging |
|---|---|---|---|
| macOS arm64 | CPython 3.12 wheels and pinned checkpoints verified | All six configurations above; native Rust SigLIP preflight recorded separately | Existing text desktop build checked; multimodal packaging not implemented |
| Windows x86_64 | Direct-dependency CPython 3.12 wheel filenames, sizes and SHA-256s verified from PyPI | **Not executed** | Not verified |
| Linux x86_64 | Same metadata check; wheels have their recorded manylinux/glibc minimums | **Not executed** | Not verified |

`platform-artifacts.json` distinguishes downloadable wheels from a working application. The
new manual `.github/workflows/multimodal.yml` downloads the selected pinned model explicitly,
runs offline diagnostics, and runs the native Rust probe for SigLIP on all three OSes. It has
not been pushed or dispatched. Only the arm64 macOS Rust target is installed here; metadata
checks and a CI definition are not Windows/Linux builds. macOS x86_64, Windows ARM, Linux ARM,
accelerators, installers and offline OS-level network-denial tests are unverified.

**Decision:** keep BGE/Nomic and all text behavior. Both candidates are viable for further
local image research. SigLIP FP32 has the more direct Rust integration path and much faster
image encoding in this test; INT8 is worth further quality evaluation. EmbeddingGemma 2 stays
a candidate for its advertised broader modalities. **No production winner or default change**:
all quality results tie at an uninformative ceiling; the reviewed-data, platform, native
preprocessing and distribution-packaging gates remain open. No absence, gap, video or audio
feature can be inferred from these retrieval results.

## Additive foundation and remaining implementation gates

`invectus_core::multimodal` adds typed text/local-media inputs, declared modality capability,
a `MultimodalEmbedder` boundary and a `TextAdapter` that forwards text exactly and rejects a
mixed batch containing media before invoking a legacy encoder. Its new `MultimodalIdentity`
envelope pins text policy, all named artifacts and sorted modality preprocessing parameters;
version, artifact, runtime, dimensions and preprocessing changes create different IDs.
Source-content hashes and query/document mode control invalidation keys.

The original `EmbeddingIdentity` type, serialization, space IDs, registry, DuckDB schema,
Lance metadata, model defaults, production encoder, search and calibration code are unchanged.
Golden tests reopen temporary projects containing actual prior identity JSON. The new envelope
is deliberately **not accepted by the text vector-table or calibrated-verdict APIs**. This
small preparatory part of phase 2 is independent of model selection; it is not a migrated media
store or a working image collection. The native example is a spike, not a model registration. The actual Mac run is retained in
`siglip-native-preflight.json`: two 768-D projected vectors with cosine 1.00000024 (text) and
0.99999946 (image) against Python ORT; rounding can put a normalized dot product slightly above
1. First-forward timings were collected during other work and are not accepted benchmarks.

Remaining gates, in order:

1. Human-adjudicated, harder retrieval data and an untouched calibration split. Record separate
   semantic and visual judgments and model failures, then make an evidence-based runtime choice.
2. Run the three-platform probes. Package a Python sidecar only if its installation/resource
   cost is acceptable, or implement and test native SigLIP tokenizer/image preprocessing against
   the reference. Resolve export license/notice handling before redistribution.
3. Add versioned media-space storage and separate image collections; copied local originals by
   default, explicit linked-source mode, full-byte hashes and source-change detection, bounded
   safe decoding, EXIF orientation, alpha compositing, thumbnails, unsupported/corrupt-file
   handling, and transactional resumable indexing with stale-vector invalidation.
4. Implement CLI and desktop image import/preview/index/text-or-image query/ranked thumbnails/
   original inspection. Prove offline operation, resumption, update/delete behavior and existing
   text compatibility with meaningful end-to-end tests. Do not substitute this Python spike for
   that deliverable.
5. Calibrate only validated collection × space × modality-pair × preprocessing scopes. The old
   text calibration schema has no modality pair and cannot be reused for media. Clustering/gaps
   need separate validation. Video/audio need actual bounded decoding and temporal/audio labels,
   not just the model card's capability listing.

## Reproduce without touching a project

Run from the repository root (CPython 3.12):

```sh
uv venv --python 3.12 .spike-data/multimodal-venv
uv pip install --python .spike-data/multimodal-venv/bin/python -r docs/multimodal/requirements-macos-arm64.txt
.spike-data/multimodal-venv/bin/python scripts/multimodal/fixtures.py --root .spike-data/multimodal-fixtures --download
.spike-data/multimodal-venv/bin/python scripts/multimodal/artifacts.py download --model gemma --root .spike-data/multimodal-models/embeddinggemma-2
INVECTUS_OFFLINE=1 .spike-data/multimodal-venv/bin/python scripts/multimodal/evaluate.py --model gemma --model-dir .spike-data/multimodal-models/embeddinggemma-2 --fixtures .spike-data/multimodal-fixtures --out .spike-data/gemma-repeat.json
```

The script uses explicit installation only. To test SigLIP, use `--model siglip`, `siglip-onnx`
or `siglip-int8` and a separate task-owned model folder. `artifacts.py verify` forces checksum
verification. Use `requirements.txt` for the other platform probes and capture `pip freeze`;
transitive wheels can differ by OS. To reproduce the native preflight:

```sh
.spike-data/multimodal-venv/bin/python scripts/multimodal/native_inputs.py --model-dir .spike-data/multimodal-models/siglip2-onnx --fixtures .spike-data/multimodal-fixtures --out .spike-data/multimodal-native-inputs.json
cargo run --locked -p invectus-embed --example multimodal_probe -- .spike-data/multimodal-models/siglip2-onnx .spike-data/multimodal-native-inputs.json .spike-data/native-repeat.json
.spike-data/multimodal-venv/bin/python -m unittest discover -s scripts/multimodal -p 'test_*.py' -v
python3 scripts/multimodal/baseline.py check docs/multimodal/baseline-data-guard.json
```

`parity.py --help` documents the isolated export/INT8 parity check. It never writes a project's
parity records. All downloaded weights, generated images, logs and probe inputs are under
ignored `.spike-data/`; tracked artifacts are small manifests, measurements and code. Repeated
runs should write new result paths instead of overwriting the recorded measurements.


## Validation checkpoint

- Baseline: 75 existing workspace tests passed before changes. After the four new contract/
  compatibility tests: **79 workspace tests passed**, including the actual installed BGE and
  CLI paths. Final rerun also passed all 79; no tests were skipped for missing model files.
- Five model-free Python tests cover duplicate-aware retrieval metrics, visible misses/invalid
  vectors, offline download refusal, checksum rejection and source-split manifest integrity.
  Cached fixture regeneration with `INVECTUS_OFFLINE=1` matches the frozen manifest.
- Rust fmt, workspace/all-target clippy with warnings denied, Python Ruff lint/format and
  frontend build passed. macOS desktop `tauri build --no-bundle --ci` passed; the known upstream
  Apple linker `__eh_frame` warning remains. Windows/Linux are still unexecuted.
- Benchmark/reference/text-model metadata guards match after the work. No source data, benchmark
  vectors, defaults, text indexes, production schema or calibrations were changed.
