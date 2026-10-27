//! Native CPU preflight for the pinned SigLIP 2 export. Does not open a project or register a
//! model. Usage: cargo run --example multimodal_probe -- <model-dir> <inputs.json> <report.json>
//! Inputs are produced by scripts/multimodal/native_inputs.py from checksum-verified fixtures.

use anyhow::{Context, Result, anyhow, ensure};
use invectus_embed::store::sha256_file;
use ort::session::Session;
use ort::session::builder::AutoDevicePolicy;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Deserialize)]
struct Inputs {
    source_id: String,
    pixel_values: Vec<f32>,
    input_ids: Vec<i64>,
    reference: BTreeMap<String, Vec<f32>>,
}

#[derive(Serialize)]
struct ResultRow {
    input_names: Vec<String>,
    output_names: Vec<String>,
    dimensions: usize,
    forward_ms: f64,
    cosine_with_python_ort: f32,
}

fn normalized(mut values: Vec<f32>) -> Result<Vec<f32>> {
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(norm.is_finite() && norm > 0.0, "invalid output norm");
    for v in &mut values {
        *v /= norm;
    }
    Ok(values)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let model_dir = PathBuf::from(args.next().context("missing model folder")?);
    let input = PathBuf::from(args.next().context("missing input file")?);
    let output = PathBuf::from(args.next().context("missing report file")?);
    ensure!(args.next().is_none(), "unexpected arguments");
    let inputs: Inputs = serde_json::from_slice(&std::fs::read(input)?)?;
    ensure!(inputs.input_ids.len() == 64, "expected 64 text tokens");
    ensure!(
        inputs.pixel_values.len() == 3 * 224 * 224,
        "expected RGB 224x224 image"
    );
    let env = ort::init().with_name("invectus-multimodal-probe").build()?;
    let mut rows = BTreeMap::new();
    for (kind, name, expected, output_name) in [
        (
            "text",
            "text_model.onnx",
            "baf12d941beabafafb14f7b4adb38dc15be18681b964a84410ec53d9d65e6293",
            "text_embeds",
        ),
        (
            "image",
            "vision_model.onnx",
            "c0573e3f4140c3a7c4e9cc5912bd6b26a033b46a6a8e8af26cbea262b163bcad",
            "image_embeds",
        ),
    ] {
        let path = model_dir.join("onnx").join(name);
        ensure!(
            sha256_file(&path)? == expected,
            "model checksum mismatch: {name}"
        );
        let mut session = Session::builder(&env)?
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(|e| anyhow!(e.to_string()))?
            .with_intra_threads(8)
            .map_err(|e| anyhow!(e.to_string()))?
            .commit_from_file(&path)?;
        let input_names = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let output_names: Vec<_> = session
            .outputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        // The separate published towers expose `pooler_output`; some combined exports use
        // modality-specific names. Never fall back to the unpooled hidden-state tensor.
        let selected_output = if output_names.iter().any(|name| name == output_name) {
            output_name
        } else {
            ensure!(
                output_names.iter().any(|name| name == "pooler_output"),
                "no projected pooled output: {output_names:?}"
            );
            "pooler_output"
        };
        let t = Instant::now();
        let result = if kind == "text" {
            session.run(
                ort::inputs!["input_ids"=>Tensor::from_array(([1,64],inputs.input_ids.clone()))?],
            )?
        } else {
            session.run(ort::inputs!["pixel_values"=>Tensor::from_array(([1,3,224,224],inputs.pixel_values.clone()))?])?
        };
        let forward_ms = t.elapsed().as_secs_f64() * 1000.0;
        let (shape, values) = result[selected_output].try_extract_tensor::<f32>()?;
        ensure!(
            shape.as_ref() == [1, 768],
            "unexpected output shape {shape:?}"
        );
        let actual = normalized(values.to_vec())?;
        let reference = normalized(
            inputs
                .reference
                .get(kind)
                .context("missing reference")?
                .clone(),
        )?;
        ensure!(
            actual.len() == reference.len(),
            "reference dimension mismatch"
        );
        let cosine_with_python_ort = actual
            .iter()
            .zip(&reference)
            .map(|(a, b)| a * b)
            .sum::<f32>();
        ensure!(
            cosine_with_python_ort >= 0.99999,
            "native output differs from Python ORT"
        );
        rows.insert(
            kind,
            ResultRow {
                input_names,
                output_names,
                dimensions: actual.len(),
                forward_ms,
                cosine_with_python_ort,
            },
        );
    }
    let report = serde_json::json!({"scope":"single-source CPU preflight, not retrieval quality or cross-platform parity",
        "source_id":inputs.source_id,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "runtime":"ort 2.0.0-rc.15, PreferCPU, 8 intra threads","results":rows});
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
