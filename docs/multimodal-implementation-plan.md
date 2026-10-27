# Multimodal search implementation plan

## Goal and timing

Extend inVectus to search across text, images, and eventually video and audio while retaining local-first operation, traceable evidence, and the existing text workflows. A user should be able to import media, search with text or media, and inspect the closest results in context. A lack of matches is evidence about the searched collection, not proof of global absence.

The gapmap benchmark and per-term ablation were completed and recorded at starting revision `b5f6d71` (confirmed 2026-10-27); no overlapping benchmark processes were found. Preserve their data, indexes and results. The gap ranking still trails raw-count and density baselines. A model swap is not a fix for that finding. Check the ledger and working tree again before subsequent phases.

## Execution checkpoint — 2026-10-27

See [the spike report and reproduction commands](multimodal/README.md), its raw measurements,
artifact/platform manifests, and `PROGRESS.md` for evidence. This checkpoint qualifies the
original roadmap; incomplete gates remain incomplete.

| Phase | Status and remaining gate |
|---|---|
| 0: baseline/evaluation | Baseline complete: clean tracked tree at `b5f6d71`, only this plan untracked, 75 tests passed, benchmark/ablation complete. A reproducible 12-source-group licensed diagnostic set exists. **Human relevance adjudication and a larger independent calibration set remain missing.** |
| 1: model/runtime | EmbeddingGemma 2, SigLIP 2 PyTorch/ONNX FP32/INT8 and BGE/Nomic text diagnostics measured offline on macOS arm64. Native Rust SigLIP towers preflighted. Artifact revisions/checksums and direct-dependency wheels verified. **No quality winner; Windows/Linux execution and product packaging remain unverified.** |
| 2: foundation | Preparatory typed inputs, capability interface, legacy text adapter and separate versioned media identity added and tested. **No persisted media schema, production model installation, media job or migrated vector table yet.** |
| 3: image application | **Gated, not implemented.** Python retrieval diagnostics and the native tower preflight do not fulfill desktop/CLI image import, resumable indexing, thumbnails or original inspection. |
| 4: calibration/gaps | **Gated.** No visual or cross-modal calibration, verdicts or gap claims enabled. |
| 5: video/audio | **Gated.** Model-card capabilities/configuration inspected only; decoding, temporal coverage, audio understanding and quality untested. |

Material decisions and corrections:

- All diagnostic configurations reached perfect hit@1/recall@5/MRR on this tiny set. Its
  agent-authored expectations are explicitly unreviewed; these ceiling results cannot pass
  the human-judgment/model-selection gate. Do not describe the held-out source split as a
  validated calibration set. Freeze stronger human-reviewed semantic/visual judgments before
  selecting a production runtime or exposing image search. Official Winoground returned
  HTTP 401 and requires research-only access terms; MMEB-eval provides an alternative but its
  referenced image archive is 7.13 GB. Record a bounded subset and constituent source terms
  rather than bulk-downloading it on the current low-headroom host.
- EmbeddingGemma 2 really has a released ungated Apache-2.0 safetensors checkpoint, but its
  pinned repository has **no ONNX artifact**. The executed path is recent Python libraries,
  FP32 on CPU, with torchvision required by the processor. FP16 is unsafe per the model docs.
  Its image encoding measured about 695 ms/query here; video/audio advertising does not
  establish a supported packaged native path or validated coverage.
- SigLIP 2 has actual separate ONNX towers and projected `pooler_output`, unlike the current
  text encoder's rank-3 hidden-state contract. It is a practical further-research candidate:
  roughly 33 ms/image query FP32, 20 ms INT8 in the diagnostic. The export card omits license
  metadata; retain upstream Apache-2.0 terms/provenance and resolve distribution notices.
  INT8 drift (minimum cosine 0.99556 text / 0.96141 image against PyTorch) requires a **new
  space**, despite unchanged smoke rankings. No production selection or default change.
- Wheels for CPython 3.12 on macOS arm64, Windows x86_64 and Linux x86_64 were verified from
  release metadata. Only macOS was executed. A manual three-OS probe workflow is provided;
  defining it is not a successful platform check. No push or remote run was made.
- A small additive part of phase 2 is safe independent work while the data/platform gates
  remain open: define contracts without routing production media through them. Keep the old
  `EmbeddingIdentity` serialization/hash untouched. Use a **separate versioned envelope** for
  new media spaces, with every artifact and modality preprocessing policy hashed. Golden tests
  cover the exact existing BGE, Nomic and foreign-runtime Nomic identities and project reopening.
  The new envelope is not accepted by legacy vector/calibration APIs; no schema migration occurs.
- Media storage/indexing and CLI/desktop images must follow the remaining gates rather than
  shipping a Python spike as the image product. Initial storage design: private copied originals
  by default; linked originals only through an explicit mode with full-byte hash rechecks.
  Thumbnail paths or file extensions are not source identities. Implement bounded decoding,
  orientation/alpha rules, content-change invalidation, resumption and source inspection together.
- The existing calibration key has no modality pair, and neither text calibration nor text gap
  thresholds transfer to media. Keep retrieval uncalibrated until collection/space/modality-pair/
  preprocessing-specific held-out validation exists. Video/audio remain separate evidence gates.

## Current starting point

- `crates/core/src/embedder.rs` defines a text-only embedder; `crates/embed` runs pinned text models through ONNX Runtime. BGE-small is the provisional default; Nomic is also supported.
- `crates/ingest` imports CSV/TSV, Parquet, and text folders. Item content, indexing jobs, and searches currently assume text.
- `EmbeddingIdentity` and vector-table metadata distinguish spaces, which provides a foundation for adding a model without mixing its vectors with BGE or Nomic. The identity, document formatting, input hashes, and parity rules currently assume text and need careful evolution.
- Existing search calibration, clustering, gap scans, and backtests are based on text collections. None should be assumed valid for visual or cross-modal comparisons.

## Phase 0 — Establish the baseline

1. Read the completed benchmark and ablation results, repository status, and the latest implementation notes. Record the current revision, supported platforms, resource use, and existing text test results.
2. Preserve the existing benchmark projects and their model identities. Do not modify reference data or re-embed the full benchmark as part of initial development.
3. Pick a small, redistributable evaluation set with a manifest and human relevance judgments for text-to-text, image-to-image, and text-to-image retrieval. Include visually similar and dissimilar items, near duplicates and meaningful transformations, and cases where visual and semantic similarity diverge. Avoid restricting evaluation to a single use case. Hold out a separate set for calibration.

**Exit:** baseline and evaluation data are reproducible, with source and license recorded, and Claude's overlapping changes are complete.

## Phase 1 — Model and runtime spike

1. Evaluate EmbeddingGemma 2 as a candidate alongside a practical alternative; multimodal breadth alone does not give it priority for production. Verify actual released model artifacts, license, preprocessing, supported inference runtimes, and the exact text/image/video capabilities of each artifact. Prototype in a separate spike directory and isolated index. Never assume an ONNX export or a runtime binding works on all targets until tested.
2. Compare against the current text model for text retrieval and at least one practical image/text baseline, such as SigLIP 2, for image retrieval. Use identical judgments where applicable. Measure recall@k and missed matches, with per-task results; report latency, throughput, peak memory, disk footprint, and quality on representative CPU hardware. Measure accelerator paths separately when available.
3. Check macOS, Windows, and Linux runtime availability, packaging, offline operation, distribution terms, model installation and checksums, and whether cross-platform outputs need a parity check. Determine which platforms actually run the model rather than inferring support from a library's name.
4. Test cross-modal compatibility using the model's required query/document instructions, preprocessing, and normalization. Verify that text and media inputs genuinely share a compatible embedding space. Record image size and video duration/frame limits, and evaluate quantization rather than assuming it preserves retrieval quality.
5. Select a model and runtime only after reviewing quality, integration cost, platform coverage, and resource measurements. If EmbeddingGemma 2 cannot meet the requirements, retain existing text functionality and document the best viable alternative or limitation.

**Exit:** a reproducible spike report with model/runtime choice, pinned revisions, per-platform evidence, results and known limitations. No default model changes yet.

## Phase 2 — Media and embedding foundation

1. Introduce typed input representations for text and media, and an embedding interface that exposes supported modalities and query/document behavior. Retain an adapter for existing `TextEmbedder` implementations so current commands and collections continue to work.
2. Evolve the persisted identity in a versioned, backward-compatible way. Identify the shared embedding space and all relevant artifacts, tokenizer and processors, pooling/normalization, instructions, precision, runtime, and modality-specific preprocessing (including image transformations and video sampling). Compatibility must be established explicitly, not inferred from vector dimensionality. Preserve old serialized identities and their space IDs; test migration and reopening old projects.
3. Extend item/source storage with media references and metadata, content hashes, parent/segment relationships, and a clear policy for copied versus linked files. Keep originals private and local by default. Make updates and deletes invalidate only affected derived data. Define deterministic handling of unsupported, unreadable, changed, and oversized files.
4. Make embedding and indexing jobs resumable and modality-aware. Keep spaces isolated, preserve existing indexes, and provide progress and actionable errors. Index into a new space; never compare or silently reuse existing BGE/Nomic vectors for the new model.
5. Add model artifact installation with pinned revisions and checksums, explicit download, offline enforcement, and per-platform errors. Do not require a network connection for local inference.

**Exit:** existing text tests and old projects still work; new typed inputs, identity compatibility, resumption, and data-change behavior have meaningful tests. No visual absence claims are enabled.

## Phase 3 — Image retrieval end to end

1. Import common image formats from a local folder with a preview and explicit handling for orientation, transparency, corrupt files, and duplicate or changed content. Store source references and safe thumbnails; avoid exporting source content by default.
2. Embed image and text inputs in the selected compatible space. Support image-to-image and text-to-image queries, and image-to-text only where the model and collection support it. Surface unsupported combinations clearly.
3. Show ranked visual results with thumbnails, similarity as a retrieval signal, source and collection context, and clear access to the original. Search covers the chosen collection unless an explicit connector adds other sources.
4. Evaluate retrieval against the held-out judgments, including the cases where users care about visual rather than semantic similarity. Record failure modes and measure latency with realistic collections.

**Exit:** an offline image collection can be imported, indexed, searched with both image and text, and inspected in the desktop app and CLI. Retrieval quality and resource measurements are documented. Results do not claim a design or idea is globally unique.

## Phase 4 — Evidence, calibration, and gap features

1. Define the meaning of a match for each supported task and collection. Build labeled positive and hard-negative pairs appropriate to each query/target modality, including image-to-image and text-to-image. Split training and held-out data to prevent leakage.
2. Fit and validate calibration separately by model, collection, modality pair, and relevant preprocessing policy. Report sample size, evaluation metrics, and uncertainty. Invalidate calibration when the underlying space or collection changes enough to require it.
3. Return ranked evidence and an explicitly uncalibrated status when suitable validation is absent. Enable existence/near/open verdicts only for a modality pair that passes documented validation; do not present similarity scores as probabilities.
4. Investigate multimodal clustering and gap scans separately. Test whether roles, dates, density, and region explanations retain meaning for each collection; avoid projecting the current text-based gap thresholds onto visual spaces. Preserve the evidence and human verification workflow.

**Exit:** any supported verdict is backed by held-out evidence and an appropriate calibration; unsupported tasks remain retrieval-only. Existing text benchmark results remain independently reproducible.

## Phase 5 — Video and audio

1. Define the user-facing unit of retrieval: a whole asset, a time-bounded segment, or both. Inspect the chosen model's real input limits and decide whether video uses native clip inputs, sampled frames, audio, or a combination. Record the sampling and processing policy in the index identity.
2. Add local decoding and bounded, resumable segment processing; retain source identity and timestamps. Make indexing cost and sampled coverage visible, and handle long, silent, audio-only, and corrupt files explicitly.
3. Return playable or inspectable source segments with timestamps and relevant modality context. Evaluate on labeled moment-retrieval and cross-modal queries, including events between sampled frames, before making any absence claims.
4. Extend calibration only where labeled data and validation support it. Treat audio and temporal understanding as separate capabilities to verify, not automatic consequences of image search.

**Exit:** supported media queries retrieve inspectable time-stamped segments, with known coverage and documented measured limitations.

## Release and decision gates

- Keep BGE/Nomic available; change a default only after an explicit comparison shows a net benefit for existing text users and the supported platforms. Re-embedding is an explicit, resumable migration into a new index, with storage/time estimates and a path to retain or remove the old index.
- Run appropriate workspace tests, format and lint checks, and desktop build/CI on supported operating systems. Include compatibility tests for old projects, offline operation, preprocessing determinism, index separation, and model installation.
- Document what was actually searched, supported modalities/platforms, performance, calibration status, and how to inspect results. Do not suggest global novelty or legal clearance from a local similarity search.
- Reassess the roadmap after each phase rather than assuming that image retrieval success proves video, audio, or gap detection quality.

## References

- Google, [EmbeddingGemma 2 model card](https://ai.google.dev/gemma/docs/embeddinggemma/model_card_2) and [multimodal guide](https://ai.google.dev/gemma/docs/embeddinggemma/multimodal-embeddinggemma-with-sentence-transformers). Verify current artifacts and platform support during the spike.
- Project sources: `SPEC.md`, `PROGRESS.md`, `crates/core/src/embedder.rs`, `crates/core/src/identity.rs`, `crates/embed`, `crates/ingest`, and `crates/index`.
