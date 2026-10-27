//! Local language models (SPEC §7.2): a pinned registry, checksum-verified installation and
//! offline generation through llama.cpp. Used for region labels, calibration paraphrases and
//! midpoint descriptions; every output records the model that produced it.
pub mod generate;
pub mod registry;
pub mod store;

pub use generate::{GenOptions, Generation, LlamaGenerator, LlmIdentity, TextGenerator};
pub use registry::{DEFAULT_LLM, LlmSpec, MODELS};
