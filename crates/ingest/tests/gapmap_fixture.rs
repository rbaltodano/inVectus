//! gapmap import against a synthetic fixture with the reference schema (no reference data).

use arrow_array::{
    Array, FixedSizeListArray, Int64Array, RecordBatch, StringArray, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use invectus_core::Project;
use invectus_index::VectorStore;
use invectus_ingest::gapmap::{GapmapPaths, import_gapmap};
use std::sync::Arc;

const DIM: usize = 8;

fn fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data/vectors")).unwrap();
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::write(
        root.join("config/models.yaml"),
        "candidates:\n  nomic:\n    kind: st\n    repo: nomic-ai/nomic-embed-text-v1.5\n    prefix: 'clustering: '\n    revision: e9b6763023c676ca8431644204f50c2b100d9aab\n    max_length: 512\n",
    )
    .unwrap();
    let c = duckdb::Connection::open(root.join("data/gapmap.duckdb")).unwrap();
    c.execute_batch(
        "CREATE TABLE papers (pmid BIGINT PRIMARY KEY, doi TEXT, title TEXT, abstract TEXT,
           year INTEGER, pub_date DATE, journal TEXT, pub_types TEXT[], is_synthesis BOOLEAN,
           is_primary BOOLEAN, is_narrative_review BOOLEAN, is_case_report BOOLEAN,
           mesh_major TEXT[]);
         INSERT INTO papers VALUES
          (11, NULL, 'Primary study', 'Abstract one.', 2020, '2020-03-04', 'J', ['Journal Article'], false, true, false, false, ['Myopia']),
          (12, NULL, 'A review', 'Abstract two.', 2021, NULL, 'J', ['Systematic Review'], true, false, false, false, []),
          (13, NULL, 'Editorial', NULL, 2019, NULL, 'J', ['Editorial'], false, false, false, false, []);",
    )
    .unwrap();
    drop(c);
    let pmids = [11i64, 12, 13, 99];
    let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        pmids.iter().map(|p| {
            let mut v = vec![0.0f32; DIM];
            v[(*p as usize) % DIM] = 1.0;
            Some(v.into_iter().map(Some))
        }),
        DIM as i32,
    );
    let schema = Arc::new(Schema::new(vec![
        Field::new("pmid", DataType::Int64, false),
        Field::new("model_id", DataType::Utf8, false),
        Field::new("vector", vectors.data_type().clone(), true),
        Field::new("input_hash", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(pmids.to_vec())),
            Arc::new(StringArray::from(vec!["nomic@e9b6763023c6"; 4])),
            Arc::new(vectors),
            Arc::new(StringArray::from(vec!["h"; 4])),
        ],
    )
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = lancedb::connect(root.join("data/vectors").to_str().unwrap())
            .execute()
            .await
            .unwrap();
        db.create_table("embeddings_nomic", batch)
            .execute()
            .await
            .unwrap();
    });
}

fn listing(root: &std::path::Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                out.push((
                    e.path().display().to_string(),
                    m.len(),
                    m.modified().unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn imports_papers_roles_and_vectors_read_only_and_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("gapmap");
    fixture(&src);
    let before = listing(&src);
    let mut p = Project::create(&dir.path().join("p")).unwrap();
    let v = VectorStore::open(&p).unwrap();
    let paths = GapmapPaths::from_root(&src).unwrap();

    let r = import_gapmap(&mut p, &v, &paths, "gapmap", None, "tok").unwrap();
    assert_eq!(r.vectors_read, 4);
    assert_eq!(r.vectors_without_paper, 1);
    assert_eq!((r.items_inserted, r.vectors_added), (3, 3));
    assert_eq!(r.roles.get("coverage"), Some(&1));
    assert_eq!(r.roles.get("demand"), Some(&1));
    assert_eq!(r.roles.get("neutral"), Some(&1));

    let table = invectus_index::select_table(&p, "gapmap", None).unwrap();
    assert_eq!(table.origin, "imported");
    assert_eq!(table.identity.document_prefix, "clustering: ");
    assert_eq!(table.identity.compute_precision, "fp16");
    let items = p
        .store
        .items_by_ids(&[invectus_core::item::item_id("gapmap", "11", None)])
        .unwrap();
    let it = items.values().next().unwrap();
    assert_eq!(it.date.as_deref(), Some("2020-03-04"));
    assert_eq!(
        it.url.as_deref(),
        Some("https://pubmed.ncbi.nlm.nih.gov/11/")
    );
    assert_eq!(it.meta["mesh_major"], serde_json::json!(["Myopia"]));

    let r = import_gapmap(&mut p, &v, &paths, "gapmap", None, "tok").unwrap();
    assert_eq!(
        (r.items_unchanged, r.vectors_unchanged, r.vectors_added),
        (3, 3, 0)
    );
    assert_eq!(listing(&src), before, "the source project was not modified");

    // --limit reads only the first N vectors.
    let mut limited = Project::create(&dir.path().join("q")).unwrap();
    let v2 = VectorStore::open(&limited).unwrap();
    let r = import_gapmap(&mut limited, &v2, &paths, "gapmap", Some(2), "tok").unwrap();
    assert_eq!(r.vectors_read, 2);
}
