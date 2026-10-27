//! Text generation with a local GGUF model through llama.cpp. Offline: the model is loaded from a
//! verified local file and nothing else is read. On macOS all layers run on Metal.

use crate::registry::LlmSpec;
use crate::store::verified_path;
use anyhow::{Context, Result, anyhow, bail};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

pub const RUNTIME: &str = "llama.cpp (llama-cpp-2 0.1.160)";

/// Which model produced a text; stored with anything generated (SPEC §6.3 step 5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmIdentity {
    pub model_key: String,
    pub repo: String,
    pub revision: String,
    pub sha256: String,
    pub runtime: String,
}

#[derive(Clone, Debug)]
pub struct GenOptions {
    pub max_tokens: usize,
    /// 0 = greedy (deterministic).
    pub temperature: f32,
    pub seed: u32,
}

impl Default for GenOptions {
    fn default() -> Self {
        Self {
            max_tokens: 400,
            temperature: 0.0,
            seed: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Generation {
    pub text: String,
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub seconds: f64,
    /// Stopped at `max_tokens` rather than at the end of the answer.
    pub truncated: bool,
}

/// A loaded language model.
pub trait TextGenerator {
    fn identity(&self) -> &LlmIdentity;
    fn generate(
        &mut self,
        system: Option<&str>,
        user: &str,
        opts: &GenOptions,
    ) -> Result<Generation>;
}

/// The prompt Gemma 4's own chat template produces for one optional system turn and one user
/// turn, with thinking disabled (an empty thought channel), minus the BOS token the tokenizer
/// adds.
pub fn gemma4_prompt(system: Option<&str>, user: &str) -> String {
    let mut p = String::new();
    if let Some(s) = system {
        p.push_str(&format!("<|turn>system\n{}<turn|>\n", s.trim()));
    }
    p.push_str(&format!(
        "<|turn>user\n{}<turn|>\n<|turn>model\n<|channel>thought\n<channel|>",
        user.trim()
    ));
    p
}

/// llama.cpp allows one backend per process.
fn backend() -> Result<&'static LlamaBackend> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let mut b = LlamaBackend::init().map_err(|e| anyhow!("initializing llama.cpp: {e}"))?;
    b.void_logs();
    Ok(BACKEND.get_or_init(|| b))
}

pub struct LlamaGenerator {
    model: LlamaModel,
    template: LlamaChatTemplate,
    /// The model's template uses Gemma 4's `<|turn>` format.
    gemma4: bool,
    identity: LlmIdentity,
    context: u32,
    threads: i32,
}

impl LlamaGenerator {
    /// Load the installed, checksum-verified model `spec` from `root`.
    pub fn load(root: &Path, spec: &LlmSpec, threads: usize) -> Result<Self> {
        let path = verified_path(root, spec)?;
        let params = if cfg!(target_os = "macos") {
            LlamaModelParams::default().with_n_gpu_layers(999)
        } else {
            LlamaModelParams::default()
        };
        let model = LlamaModel::load_from_file(backend()?, &path, &params)
            .with_context(|| format!("loading {}", path.display()))?;
        let template = model
            .chat_template(None)
            .context("the model has no built-in chat template")?;
        let gemma4 = template
            .to_str()
            .is_ok_and(|t| t.contains("<|turn>") && t.contains("<turn|>"));
        Ok(Self {
            model,
            template,
            gemma4,
            identity: LlmIdentity {
                model_key: spec.key.into(),
                repo: spec.repo.into(),
                revision: spec.revision.into(),
                sha256: spec.sha256.into(),
                runtime: RUNTIME.into(),
            },
            context: spec.context,
            threads: threads.max(1) as i32,
        })
    }
}

impl TextGenerator for LlamaGenerator {
    fn identity(&self) -> &LlmIdentity {
        &self.identity
    }

    fn generate(
        &mut self,
        system: Option<&str>,
        user: &str,
        opts: &GenOptions,
    ) -> Result<Generation> {
        let started = Instant::now();
        let mut messages = Vec::new();
        if let Some(s) = system {
            messages.push(LlamaChatMessage::new("system".into(), s.into())?);
        }
        messages.push(LlamaChatMessage::new("user".into(), user.into())?);
        let prompt = match self
            .model
            .apply_chat_template(&self.template, &messages, true)
        {
            Ok(p) => p,
            // llama.cpp's built-in template engine does not know Gemma 4's turn format yet.
            Err(_) if self.gemma4 => gemma4_prompt(system, user),
            Err(e) => return Err(e).context("applying the model's chat template"),
        };
        let vocab = self.model.vocab();
        let tokens = vocab.tokenize(prompt.as_bytes(), true, true);
        if tokens.is_empty() {
            bail!("empty prompt");
        }
        if tokens.len() + opts.max_tokens > self.context as usize {
            bail!(
                "prompt of {} tokens plus {} to generate exceeds the {}-token context",
                tokens.len(),
                opts.max_tokens,
                self.context
            );
        }
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.context))
            .with_n_batch(self.context)
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads);
        let mut ctx = self.model.new_context(backend()?, params)?;
        let mut batch = LlamaBatch::new(self.context as usize, 1);
        let last = tokens.len() - 1;
        for (i, t) in tokens.iter().enumerate() {
            batch.add(*t, i as i32, &[0], i == last)?;
        }
        ctx.decode(&mut batch)?;
        let mut sampler = if opts.temperature <= 0.0 {
            LlamaSampler::greedy()
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::top_k(40),
                LlamaSampler::top_p(0.95, 1),
                LlamaSampler::temp(opts.temperature),
                LlamaSampler::dist(opts.seed),
            ])
        };
        let mut out: Vec<u8> = Vec::new();
        let mut position = tokens.len() as i32;
        let mut generated = 0;
        let mut truncated = true;
        while generated < opts.max_tokens {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);
            // Gemma 4 ends a turn with a control token that is not always flagged
            // end-of-generation; control tokens before any text (channel markers) are skipped.
            let control = vocab.is_control(token);
            if vocab.is_eog(token) || (control && !out.is_empty()) {
                truncated = false;
                break;
            }
            if !control {
                out.extend(vocab.token_to_piece(token, false, None));
            }
            generated += 1;
            batch.clear();
            batch.add(token, position, &[0], true)?;
            position += 1;
            ctx.decode(&mut batch)?;
        }
        Ok(Generation {
            text: String::from_utf8_lossy(&out).trim().to_string(),
            prompt_tokens: tokens.len(),
            generated_tokens: generated,
            seconds: started.elapsed().as_secs_f64(),
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemma4_prompt_matches_the_template_with_thinking_off() {
        assert_eq!(
            gemma4_prompt(Some(" Be brief. "), "Hi"),
            "<|turn>system\nBe brief.<turn|>\n<|turn>user\nHi<turn|>\n<|turn>model\n\
             <|channel>thought\n<channel|>"
        );
        assert!(gemma4_prompt(None, "Hi").starts_with("<|turn>user\n"));
    }
}
