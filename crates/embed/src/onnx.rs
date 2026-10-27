//! ONNX Runtime embedder. CPU only: `AutoDevicePolicy::PreferCPU` because ort 2.0.0-rc.15's
//! default (`MaxEfficiency`) selects CoreML on macOS even when a CPU provider is listed (P0).

use crate::registry::{self, ModelSpec};
use crate::store::ModelStore;
use anyhow::{Context, Result, anyhow, bail};
use invectus_core::{
    Embedding, EmbeddingIdentity, InputKind, Normalization, Pooling, TextEmbedder,
};
use ort::session::Session;
use ort::session::builder::{AutoDevicePolicy, GraphOptimizationLevel};
use ort::value::Tensor;
use std::borrow::Cow;
use std::sync::OnceLock;
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

pub const RUNTIME: &str =
    "onnxruntime-cpu (ort 2.0.0-rc.15, AutoDevicePolicy::PreferCPU, graph opt level3)";

#[derive(Clone, Debug)]
pub struct EmbedOptions {
    pub threads: usize,
    pub batch_size: usize,
}

impl Default for EmbedOptions {
    fn default() -> Self {
        Self {
            threads: std::thread::available_parallelism()
                .map(|n| n.get().min(8))
                .unwrap_or(4),
            batch_size: 32,
        }
    }
}

pub struct OnnxEmbedder {
    session: Session,
    tokenizer: Tokenizer,
    identity: EmbeddingIdentity,
    input_names: Vec<String>,
    output_name: String,
    batch_size: usize,
}

/// ONNX Runtime allows one environment per process; keep it alive for the process lifetime.
fn environment() -> Result<&'static ort::environment::Environment> {
    static ENV: OnceLock<ort::environment::Environment> = OnceLock::new();
    if let Some(e) = ENV.get() {
        return Ok(e);
    }
    let env = ort::init()
        .with_name("invectus")
        .build()
        .map_err(|e| anyhow!("initializing ONNX Runtime: {e}"))?;
    Ok(ENV.get_or_init(|| env))
}

impl OnnxEmbedder {
    /// Identity of vectors this registry model produces with its own prefixes and pooling.
    pub fn identity_for(spec: &ModelSpec) -> EmbeddingIdentity {
        EmbeddingIdentity {
            model_key: spec.key.into(),
            repo: spec.repo.into(),
            revision: spec.revision.into(),
            weights_sha256: spec.file("model.onnx").sha256.into(),
            tokenizer_sha256: spec.file("tokenizer.json").sha256.into(),
            document_prefix: spec.document_prefix.into(),
            query_prefix: spec.query_prefix.into(),
            text_template: "{title}. {text}".into(),
            pooling: spec.pooling,
            normalization: spec.normalization,
            max_tokens: spec.max_tokens,
            truncation: "head".into(),
            dimensions: spec.dimensions,
            compute_precision: "fp32".into(),
            storage_precision: "f32".into(),
            runtime: RUNTIME.into(),
        }
    }

    pub fn load(store: &ModelStore, key: &str, opts: &EmbedOptions) -> Result<Self> {
        let spec = registry::spec(key)?;
        Self::load_with_identity(store, spec, Self::identity_for(spec), opts)
    }

    /// Load a local encoder that reproduces another space's text policy (prefixes, template,
    /// pooling, normalization, token cap) for the same model commit, e.g. to query vectors
    /// imported from gapmap. The result is a *different* space (different weights file and
    /// runtime) and may only search the target after a recorded parity check.
    pub fn load_for_space(
        store: &ModelStore,
        target: &EmbeddingIdentity,
        opts: &EmbedOptions,
    ) -> Result<Self> {
        let spec =
            registry::spec_for_revision(&target.repo, &target.revision).with_context(|| {
                format!(
                    "no pinned local model for {}@{}; cannot embed queries for this space",
                    target.repo, target.revision
                )
            })?;
        let mut identity = Self::identity_for(spec);
        identity.document_prefix = target.document_prefix.clone();
        identity.query_prefix = target.query_prefix.clone();
        identity.text_template = target.text_template.clone();
        identity.pooling = target.pooling;
        identity.normalization = target.normalization;
        identity.max_tokens = target.max_tokens;
        if identity.tokenizer_sha256 != target.tokenizer_sha256
            && !target.tokenizer_sha256.is_empty()
        {
            bail!("the pinned tokenizer differs from the space's tokenizer");
        }
        identity.tokenizer_sha256 = target.tokenizer_sha256.clone();
        Self::load_with_identity(store, spec, identity, opts)
    }

    fn load_with_identity(
        store: &ModelStore,
        spec: &ModelSpec,
        identity: EmbeddingIdentity,
        opts: &EmbedOptions,
    ) -> Result<Self> {
        let dir = store.verified_dir(spec)?;
        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow!("loading tokenizer: {e}"))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: identity.max_tokens,
                ..Default::default()
            }))
            .map_err(|e| anyhow!(e))?;
        tokenizer.with_padding(Some(PaddingParams::default()));
        let session = Session::builder(environment()?)
            .map_err(|e| anyhow!(e.to_string()))?
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(|e| anyhow!(e.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!(e.to_string()))?
            .with_intra_threads(opts.threads.max(1))
            .map_err(|e| anyhow!(e.to_string()))?
            .commit_from_file(dir.join("model.onnx"))
            .context("loading ONNX model")?;
        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();
        for required in ["input_ids", "attention_mask"] {
            if !input_names.iter().any(|n| n == required) {
                bail!("model has no {required} input (inputs: {input_names:?})");
            }
        }
        let outputs: Vec<String> = session
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();
        let output_name = outputs
            .iter()
            .find(|n| *n == "last_hidden_state")
            .or(outputs.first())
            .context("model has no outputs")?
            .clone();
        Ok(Self {
            session,
            tokenizer,
            identity,
            input_names,
            output_name,
            batch_size: opts.batch_size.max(1),
        })
    }

    fn embed_batch(&mut self, texts: Vec<String>) -> Result<Vec<Embedding>> {
        let enc = self
            .tokenizer
            .encode_batch(texts, true)
            .map_err(|e| anyhow!("tokenizing: {e}"))?;
        let n = enc.len();
        let seq = enc[0].len();
        let flat = |f: &dyn Fn(&tokenizers::Encoding) -> &[u32]| -> Vec<i64> {
            enc.iter()
                .flat_map(|e| f(e).iter().map(|v| *v as i64))
                .collect()
        };
        let ids = flat(&|e| e.get_ids());
        let mask = flat(&|e| e.get_attention_mask());
        let types = flat(&|e| e.get_type_ids());
        let mut inputs: Vec<(Cow<'_, str>, ort::session::SessionInputValue<'_>)> = Vec::new();
        for name in &self.input_names {
            let data = match name.as_str() {
                "input_ids" => ids.clone(),
                "attention_mask" => mask.clone(),
                "token_type_ids" => types.clone(),
                other => bail!("unsupported model input {other}"),
            };
            inputs.push((
                Cow::Owned(name.clone()),
                Tensor::from_array(([n, seq], data))?.into(),
            ));
        }
        let outputs = self.session.run(inputs)?;
        let (shape, values) = outputs[self.output_name.as_str()].try_extract_tensor::<f32>()?;
        if shape.len() != 3 || shape[0] as usize != n || shape[1] as usize != seq {
            bail!("unexpected output shape {shape:?}");
        }
        let dim = shape[2] as usize;
        if dim != self.identity.dimensions {
            bail!(
                "model produced {dim} dimensions, identity says {}",
                self.identity.dimensions
            );
        }
        let vectors = pool(
            values,
            &mask,
            n,
            seq,
            dim,
            self.identity.pooling,
            self.identity.normalization,
        )?;
        Ok(vectors
            .into_iter()
            .zip(&enc)
            .map(|(vector, e)| Embedding {
                vector,
                tokens: e.get_attention_mask().iter().filter(|m| **m != 0).count(),
                truncated: !e.get_overflowing().is_empty(),
            })
            .collect())
    }
}

impl TextEmbedder for OnnxEmbedder {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }

    fn embed(&mut self, texts: &[String], kind: InputKind) -> Result<Vec<Embedding>> {
        let prefix = self.identity.prefix(kind).to_string();
        // Length-sorted batches keep padding low; results are returned in input order.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| texts[i].len());
        let mut out: Vec<Option<Embedding>> = vec![None; texts.len()];
        for idx in order.chunks(self.batch_size) {
            let batch = idx
                .iter()
                .map(|&i| format!("{prefix}{}", texts[i]))
                .collect();
            for (&i, e) in idx.iter().zip(self.embed_batch(batch)?) {
                out[i] = Some(e);
            }
        }
        Ok(out
            .into_iter()
            .map(|e| e.expect("every input embedded"))
            .collect())
    }
}

/// Pool `[n, seq, dim]` hidden states into unit vectors.
pub fn pool(
    hidden: &[f32],
    mask: &[i64],
    n: usize,
    seq: usize,
    dim: usize,
    pooling: Pooling,
    normalization: Normalization,
) -> Result<Vec<Vec<f32>>> {
    if hidden.len() != n * seq * dim || mask.len() != n * seq {
        bail!("hidden state / mask size mismatch");
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = vec![0f32; dim];
        match pooling {
            Pooling::Cls => row.copy_from_slice(&hidden[i * seq * dim..i * seq * dim + dim]),
            Pooling::Mean => {
                let m = &mask[i * seq..(i + 1) * seq];
                let count = m.iter().filter(|v| **v != 0).count();
                if count == 0 {
                    bail!("input {i} has no tokens");
                }
                for (t, _) in m.iter().enumerate().filter(|(_, v)| **v != 0) {
                    let h = &hidden[(i * seq + t) * dim..(i * seq + t + 1) * dim];
                    for (r, x) in row.iter_mut().zip(h) {
                        *r += x;
                    }
                }
                for r in &mut row {
                    *r /= count as f32;
                }
            }
        }
        if normalization == Normalization::LayerNormL2 {
            // Parameter-free layer norm then L2: the variance scale cancels, centering remains.
            let mean = row.iter().sum::<f32>() / dim as f32;
            for r in &mut row {
                *r -= mean;
            }
        }
        let norm = row.iter().map(|x| x * x).sum::<f32>().sqrt();
        if !norm.is_finite() || norm == 0.0 {
            bail!("input {i} produced a zero or non-finite vector");
        }
        for r in &mut row {
            *r /= norm;
        }
        out.push(row);
    }
    Ok(out)
}

/// Local query encoder for vectors stored with `identity`: the registry model itself when the
/// identity is exactly what this build produces, otherwise a same-model encoder that reproduces
/// the identity's text policy (admitted only after a passing parity check).
pub fn query_encoder(
    store: &ModelStore,
    identity: &EmbeddingIdentity,
    opts: &EmbedOptions,
) -> Result<OnnxEmbedder> {
    if let Ok(spec) = registry::spec(&identity.model_key)
        && OnnxEmbedder::identity_for(spec) == *identity
    {
        return OnnxEmbedder::load(store, spec.key, opts);
    }
    OnnxEmbedder::load_for_space(store, identity, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cls_and_mean_pooling_respect_the_mask() {
        // n=1, seq=3, dim=2; third token is padding and must be ignored by mean pooling.
        let hidden = [3.0, 4.0, 1.0, 0.0, 100.0, 100.0];
        let mask = [1, 1, 0];
        let cls = pool(&hidden, &mask, 1, 3, 2, Pooling::Cls, Normalization::L2).unwrap();
        assert_eq!(cls[0], vec![0.6, 0.8]);
        let mean = pool(&hidden, &mask, 1, 3, 2, Pooling::Mean, Normalization::L2).unwrap();
        // mean = (2, 2) -> normalized (0.7071, 0.7071)
        assert!((mean[0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((mean[0][1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn layer_norm_centers_before_l2() {
        let hidden = [1.0, 2.0, 3.0];
        let v = pool(
            &hidden,
            &[1],
            1,
            1,
            3,
            Pooling::Cls,
            Normalization::LayerNormL2,
        )
        .unwrap();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!((v[0][0] + s).abs() < 1e-6 && v[0][1].abs() < 1e-6 && (v[0][2] - s).abs() < 1e-6);
    }

    #[test]
    fn degenerate_inputs_are_errors() {
        assert!(pool(&[0.0, 0.0], &[1], 1, 1, 2, Pooling::Cls, Normalization::L2).is_err());
        assert!(pool(&[1.0, 0.0], &[0], 1, 1, 2, Pooling::Mean, Normalization::L2).is_err());
        assert!(pool(&[1.0], &[1], 1, 1, 2, Pooling::Cls, Normalization::L2).is_err());
    }
}
