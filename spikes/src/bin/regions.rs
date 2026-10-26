use anyhow::Result;
use hdbscan::{Hdbscan, HdbscanHyperParams};
use ndarray::Array2;
use std::{fs, time::Instant};
use umap_rs::{Umap, UmapConfig};
fn floats(path: &str) -> Result<Vec<f32>> {
    Ok(fs::read(path)?
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}
fn save(path: &str, x: impl Iterator<Item = f32>) -> Result<()> {
    fs::write(path, x.flat_map(f32::to_le_bytes).collect::<Vec<_>>())?;
    Ok(())
}
fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or("cluster".into());
    let data = floats(".spike-data/vectors.f32")?;
    let n = data.len() / 768;
    assert!((30..=10_000).contains(&n));
    if mode == "cluster" {
        let y = floats(".spike-data/python-umap-10.f32")?;
        let rows: Vec<Vec<f32>> = y.chunks_exact(10).map(|r| r.to_vec()).collect();
        let params = HdbscanHyperParams::builder()
            .min_cluster_size(30)
            .min_samples(10)
            .build();
        let t = Instant::now();
        let labels = Hdbscan::new(&rows, params).cluster()?;
        let seconds = t.elapsed().as_secs_f64();
        fs::write(".spike-data/rust-labels.json", serde_json::to_vec(&labels)?)?;
        println!(
            "{}",
            serde_json::json!({"items":n,"seconds":seconds,"clusters":labels.iter().filter(|v| **v>=0).collect::<std::collections::HashSet<_>>().len(),"noise":labels.iter().filter(|v| **v<0).count() as f64/n as f64,"min_cluster_size":30,"min_samples":10})
        );
    } else {
        let x = Array2::from_shape_vec((n, 768), data)?;
        let inds = fs::read(".spike-data/knn.u32")?
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let idx = Array2::from_shape_vec((n, 30), inds)?;
        let dists = Array2::from_shape_vec((n, 30), floats(".spike-data/knn.f32")?)?;
        for run in 0..2 {
            // Share exactly the same PCA initialization as the Python comparison.
            // Parallel SGD itself remains nondeterministic.
            let init = Array2::from_shape_vec((n, 2), floats(".spike-data/umap-init.f32")?)?;
            let mut config = UmapConfig::default();
            config.graph.n_neighbors = 30;
            config.optimization.n_epochs = Some(200);
            let t = Instant::now();
            let fitted = Umap::new(config).fit(x.view(), idx.view(), dists.view(), init.view());
            let seconds = t.elapsed().as_secs_f64();
            assert!(fitted.embedding().iter().all(|v| v.is_finite()));
            save(
                &format!(".spike-data/rust-umap-{run}.f32"),
                fitted.embedding().iter().copied(),
            )?;
            println!(
                "{}",
                serde_json::json!({"items":n,"run":run,"seconds":seconds,"epochs":200,"dimensions":2,"knn_k":30})
            );
        }
    }
    Ok(())
}
