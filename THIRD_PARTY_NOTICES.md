# Third-party notices

inVectus's original code is licensed under [MIT](LICENSE). Third-party code, fonts,
icons and separately installed models retain their own licenses.

## Vendored source

[LanceDB 0.40.0](https://github.com/lancedb/lancedb/tree/v0.40.0) is Apache-2.0,
copyright The LanceDB Authors. The vendored copy includes the [upstream license](third_party/lancedb-0.40.0/LICENSE).
Its local-only changes and pinned archive checksum are documented in
[third_party/README.md](third_party/README.md). Modified source files carry notices.

## Fonts and icons

- **Inter:** copyright The Inter Project Authors; [SIL Open Font License 1.1](docs/licenses/Inter-OFL.txt).
- **Martian Mono:** copyright The Martian Mono Project Authors; [SIL Open Font License 1.1](docs/licenses/Martian-Mono-OFL.txt).
- **Lucide:** ISC, with Feather-derived icons under MIT; the complete
  [Lucide and Feather notices](docs/licenses/Lucide.txt) are preserved.

## Dependencies and models

The exact Rust and JavaScript dependency versions are recorded in `Cargo.lock`,
`spikes/Cargo.lock` and `package-lock.json`. Their licenses are specified by their
respective upstream packages. The notices above cover the vendored source and
fonts/icons included here; they are not a complete binary-distribution license inventory.

No embedding or language model weights are included in this repository or the demo.
Model downloads and installations are explicit, pinned and checksum-verified. Review
model terms at the source before use or redistribution:

- [BGE-small-en-v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5), using the pinned
  [Xenova ONNX conversion](https://huggingface.co/Xenova/bge-small-en-v1.5).
- [Nomic Embed Text v1.5](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5).
- Local language-model sources are recorded in the [model registry](crates/llm/src/registry.rs).

The public demo generator contains original fictional text. It does not include
third-party research abstracts, private collections, or real study findings.
