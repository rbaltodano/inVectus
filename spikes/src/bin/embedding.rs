use anyhow::{Result, anyhow};
use ort::{
    session::{
        Session,
        builder::{AutoDevicePolicy, GraphOptimizationLevel},
    },
    value::Tensor,
};
use std::{fs, time::Instant};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

fn embed(
    session: &mut Session,
    tokenizer: &Tokenizer,
    texts: &[String],
    nomic: bool,
    batch: usize,
) -> Result<Vec<Vec<f32>>> {
    let mut result = Vec::new();
    for chunk in texts.chunks(batch) {
        let enc = tokenizer
            .encode_batch(chunk.to_vec(), true)
            .map_err(|e| anyhow!(e))?;
        let n = enc.len();
        let seq = enc[0].len();
        let ids: Vec<i64> = enc
            .iter()
            .flat_map(|e| e.get_ids().iter().map(|v| *v as i64))
            .collect();
        let mask: Vec<i64> = enc
            .iter()
            .flat_map(|e| e.get_attention_mask().iter().map(|v| *v as i64))
            .collect();
        let types: Vec<i64> = enc
            .iter()
            .flat_map(|e| e.get_type_ids().iter().map(|v| *v as i64))
            .collect();
        let outputs = session.run(ort::inputs![
            "input_ids" => Tensor::from_array(([n,seq],ids))?,
            "attention_mask" => Tensor::from_array(([n,seq],mask.clone()))?,
            "token_type_ids" => Tensor::from_array(([n,seq],types))?
        ])?;
        let (shape, values) = outputs[0].try_extract_tensor::<f32>()?;
        assert_eq!(shape.len(), 3);
        let dim = shape[2] as usize;
        for i in 0..n {
            let mut row = vec![0f32; dim];
            if nomic {
                let count = mask[i * seq..(i + 1) * seq].iter().sum::<i64>() as f32;
                for t in 0..seq {
                    if mask[i * seq + t] != 0 {
                        for j in 0..dim {
                            row[j] += values[(i * seq + t) * dim + j] / count;
                        }
                    }
                }
                // Full-dimensional layer norm then L2: the scalar variance cancels.
                let mean = row.iter().sum::<f32>() / dim as f32;
                for v in &mut row {
                    *v -= mean;
                }
            } else {
                row.copy_from_slice(&values[i * seq * dim..i * seq * dim + dim]);
            }
            let norm = row.iter().map(|x| x * x).sum::<f32>().sqrt();
            for v in &mut row {
                *v /= norm;
            }
            result.push(row);
        }
    }
    Ok(result)
}
fn main() -> Result<()> {
    let name = std::env::args().nth(1).unwrap_or("bge".into());
    assert!(name == "nomic" || name == "bge");
    let dir = format!(".spike-data/models/{name}");
    let mut tokenizer =
        Tokenizer::from_file(format!("{dir}/tokenizer.json")).map_err(|e| anyhow!(e))?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: 512,
            ..Default::default()
        }))
        .map_err(|e| anyhow!(e))?;
    tokenizer.with_padding(Some(PaddingParams::default()));
    let env = ort::init().build()?;
    let t = Instant::now();
    let mut session = Session::builder(&env)?
        .with_auto_device(AutoDevicePolicy::PreferCPU)
        .map_err(|e| anyhow!(e.to_string()))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow!(e.to_string()))?
        .with_intra_threads(8)
        .map_err(|e| anyhow!(e.to_string()))?
        .commit_from_file(format!("{dir}/model.onnx"))?;
    let load_s = t.elapsed().as_secs_f64();
    let raw: Vec<String> = serde_json::from_slice(&fs::read(".spike-data/texts.json")?)?;
    let texts: Vec<String> = raw
        .iter()
        .take(256)
        .map(|s| {
            if name == "nomic" {
                format!("search_document: {s}")
            } else {
                s.clone()
            }
        })
        .collect();
    let lengths: Vec<usize> = texts
        .iter()
        .map(|s| {
            tokenizer
                .encode(s.as_str(), true)
                .map(|e| e.len())
                .map_err(|e| anyhow!(e))
        })
        .collect::<Result<_>>()?;
    embed(&mut session, &tokenizer, &texts[..8], name == "nomic", 8)?;
    let mut times = Vec::new();
    let mut dim = 0;
    for _ in 0..3 {
        let t = Instant::now();
        let v = embed(&mut session, &tokenizer, &texts, name == "nomic", 32)?;
        times.push(t.elapsed().as_secs_f64());
        dim = v[0].len();
        assert!(v.iter().flatten().all(|x| x.is_finite()));
        assert!(
            v.iter()
                .all(|r| (r.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 0.001)
        );
    }
    println!(
        "{}",
        serde_json::json!({"model":name,"items":texts.len(),"dim":dim,"load_s":load_s,"seconds":times,"items_per_s":times.iter().map(|s|texts.len() as f64/s).collect::<Vec<_>>(),"threads":8,"batch":32,"max_tokens":512,"provider":"CPU, AutoDevicePolicy::PreferCPU","mean_tokens_capped":lengths.iter().sum::<usize>() as f64/lengths.len() as f64,"at_512_tokens":lengths.iter().filter(|n|**n==512).count()})
    );
    Ok(())
}
