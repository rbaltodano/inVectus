use anyhow::Result;
use arrow_array::{Array, FixedSizeListArray, Int32Array, RecordBatch, types::Float32Type};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    DistanceType,
    index::{Index, vector::IvfFlatIndexBuilder},
    query::{ExecutableQuery, QueryBase},
};
use std::{fs, sync::Arc, time::Instant};

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
#[tokio::main]
async fn main() -> Result<()> {
    let raw = fs::read(".spike-data/vectors.f32")?;
    let values: Vec<f32> = raw
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let x: Vec<Vec<f32>> = values.chunks_exact(768).map(|r| r.to_vec()).collect();
    assert!((256..=10_000).contains(&x.len()));
    let path = format!(".spike-data/lance-{}", std::process::id());
    let db = lancedb::connect(&path).execute().await?;
    let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        x.iter().map(|r| Some(r.iter().copied().map(Some))),
        768,
    );
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("vector", vectors.data_type().clone(), true),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int32Array::from_iter_values(0..x.len() as i32)),
            Arc::new(vectors),
        ],
    )?;
    let t = Instant::now();
    let table = db.create_table("sample", batch).execute().await?;
    let create_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    table
        .create_index(
            &["vector"],
            Index::IvfFlat(
                IvfFlatIndexBuilder::default()
                    .num_partitions(16)
                    .distance_type(DistanceType::Cosine),
            ),
        )
        .execute()
        .await?;
    let index_s = t.elapsed().as_secs_f64();
    let indices = table.list_indices().await?;
    assert!(!indices.is_empty());
    let plan = table
        .query()
        .nearest_to(x[0].as_slice())?
        .distance_type(DistanceType::Cosine)
        .nprobes(4)
        .limit(50)
        .explain_plan(false)
        .await?;
    fs::write("docs/spikes/lance-query-plan.txt", &plan)?;
    assert!(
        plan.contains("ANN") || plan.contains("Ivf"),
        "expected indexed plan: {plan}"
    );
    for probes in [4, 16] {
        let mut times = Vec::new();
        let mut recall = 0.;
        let mut rescore_times = Vec::new();
        for i in 0..50 {
            let q = &x[i * 37 % x.len()];
            let t = Instant::now();
            let batches: Vec<RecordBatch> = table
                .query()
                .nearest_to(q.as_slice())?
                .distance_type(DistanceType::Cosine)
                .nprobes(probes)
                .limit(50)
                .execute()
                .await?
                .try_collect()
                .await?;
            times.push(t.elapsed().as_secs_f64() * 1000.);
            let ids: Vec<usize> = batches
                .iter()
                .flat_map(|b| {
                    b.column_by_name("id")
                        .unwrap()
                        .as_any()
                        .downcast_ref::<Int32Array>()
                        .unwrap()
                        .values()
                        .iter()
                        .map(|v| *v as usize)
                })
                .collect();
            let t = Instant::now();
            let mut scored: Vec<_> = ids.iter().map(|j| (*j, dot(q, &x[*j]))).collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1));
            rescore_times.push(t.elapsed().as_secs_f64() * 1000.);
            let mut exact: Vec<_> = x.iter().enumerate().map(|(j, v)| (j, dot(q, v))).collect();
            exact.sort_by(|a, b| b.1.total_cmp(&a.1));
            recall += exact[..10]
                .iter()
                .filter(|(j, _)| scored[..10].iter().any(|(id, _)| id == j))
                .count() as f64
                / 10.;
        }
        times.sort_by(f64::total_cmp);
        rescore_times.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"items":x.len(),"dim":768,"create_s":create_s,"index_s":index_s,"index":"IVF_FLAT cosine, 16 partitions", "probes":probes,"queries":50,"candidates":50,"ann_p50_ms":times[25],"ann_p95_ms":times[47],"rescore_p50_ms":rescore_times[25],"recall_at_10":recall/50.,"index_count":indices.len()})
        );
    }
    drop(table);
    drop(db);
    fs::remove_dir_all(path)?;
    Ok(())
}
