use invectus_core::{Project, Role};
use invectus_ingest::{
    ChunkPolicy, ImportRequest, Mapping, MetaMapping, RoleMapping, Source, import, preview,
};
use std::path::Path;

fn csv(dir: &Path) -> std::path::PathBuf {
    let p = dir.join("papers.csv");
    std::fs::write(
        &p,
        "pmid,title,abstract,year,pub_types,journal\n\
         1,Ortho-k and myopia,Orthokeratology slows axial growth.,2019,[Journal Article],Optom\n\
         2,Review of ortho-k,A systematic review of orthokeratology.,2021,\"[Journal Article, Systematic Review]\",Optom\n\
         3,No abstract,,2020,[Journal Article],Optom\n\
         1,Duplicate,Duplicate row.,2019,[Journal Article],Optom\n\
         4,Undated,Some text.,circa 2000,[Editorial],Other\n",
    )
    .unwrap();
    p
}

fn mapping() -> Mapping {
    Mapping {
        id: Some("pmid".into()),
        title: Some("title".into()),
        text: vec!["abstract".into()],
        date: Some("year".into()),
        url: None,
        role: RoleMapping::Column {
            column: "pub_types".into(),
            rules: RoleMapping::parse_rules("Systematic Review=coverage,Journal Article=demand")
                .unwrap(),
            default: Role::Neutral,
        },
        meta: MetaMapping::All,
    }
}

fn request(source: Source) -> ImportRequest {
    ImportRequest {
        collection: "papers".into(),
        source,
        mapping: mapping(),
        chunking: ChunkPolicy::default(),
        limit: None,
    }
}

#[test]
fn csv_import_maps_roles_skips_bad_rows_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create(&dir.path().join("p")).unwrap();
    let req = request(Source::detect(&csv(dir.path())).unwrap());

    let pv = preview(&req, 20).unwrap();
    assert_eq!(pv.rows.len(), 4, "preview maps rows but writes nothing");
    assert!(p.store.collection("papers").unwrap().is_none());

    let r = import(&mut p, &req).unwrap();
    assert_eq!(r.source_rows, 5);
    assert_eq!((r.items, r.inserted, r.updated, r.unchanged), (3, 3, 0, 0));
    assert_eq!(r.skipped.get("empty text"), Some(&1));
    assert_eq!(r.skipped.get("duplicate id"), Some(&1));
    assert!(r.warnings.iter().any(|w| w.contains("could not be parsed")));
    let s = &p.store.collection_summaries().unwrap()[0];
    assert_eq!(s.by_role.get("coverage"), Some(&1));
    assert_eq!(s.by_role.get("demand"), Some(&1));
    assert_eq!(s.by_role.get("neutral"), Some(&1));

    let r = import(&mut p, &req).unwrap();
    assert_eq!(
        (r.inserted, r.updated, r.unchanged),
        (0, 0, 3),
        "re-import is a no-op"
    );

    // Editing a row updates exactly that item; dropping one is reported, not deleted.
    std::fs::write(
        dir.path().join("papers.csv"),
        "pmid,title,abstract,year,pub_types,journal\n\
         1,Ortho-k and myopia,Orthokeratology slows axial growth in children.,2019,[Journal Article],Optom\n\
         2,Review of ortho-k,A systematic review of orthokeratology.,2021,\"[Journal Article, Systematic Review]\",Optom\n",
    )
    .unwrap();
    let r = import(&mut p, &req).unwrap();
    assert_eq!(
        (r.inserted, r.updated, r.unchanged, r.not_in_source),
        (0, 1, 1, 1)
    );

    // Changing the chunk policy of an existing collection is refused.
    let mut other = req.clone();
    other.chunking.max_chars = 500;
    other.chunking.overlap_chars = 50;
    assert!(import(&mut p, &other).is_err());
}

#[test]
fn parquet_import_matches_csv() {
    let dir = tempfile::tempdir().unwrap();
    let csv_path = csv(dir.path());
    let pq = dir.path().join("papers.parquet");
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "COPY (SELECT * FROM read_csv('{}', header=true, all_varchar=true)) TO '{}' (FORMAT PARQUET)",
        csv_path.display(),
        pq.display()
    ))
    .unwrap();
    let mut p = Project::create(&dir.path().join("p")).unwrap();
    let r = import(&mut p, &request(Source::detect(&pq).unwrap())).unwrap();
    assert_eq!((r.items, r.inserted), (3, 3));
    let rows = p.store.embed_rows("papers").unwrap();
    assert!(
        rows.iter()
            .any(|r| r.text == "A systematic review of orthokeratology.")
    );
}

#[test]
fn long_text_is_chunked_and_folder_sources_import() {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("short.md"), "# Short note\nA few words.").unwrap();
    let long = "Paragraph about bridges between ideas. ".repeat(120);
    std::fs::write(notes.join("long.txt"), &long).unwrap();
    let mut p = Project::create(&dir.path().join("p")).unwrap();
    let req = ImportRequest {
        collection: "notes".into(),
        source: Source::detect(&notes).unwrap(),
        mapping: Mapping::folder(RoleMapping::default()),
        chunking: ChunkPolicy::default(),
        limit: None,
    };
    let r = import(&mut p, &req).unwrap();
    assert_eq!((r.items, r.items_chunked), (2, 1));
    assert!(r.chunks_written >= 2);
    let rows = p.store.embed_rows("notes").unwrap();
    let chunks: Vec<_> = rows.iter().filter(|r| r.parent_id.is_some()).collect();
    assert_eq!(chunks.len(), r.chunks_written);
    assert!(chunks.iter().all(|c| c.text.chars().count() <= 2000));
    let summary = &p.store.collection_summaries().unwrap()[0];
    assert_eq!(
        (summary.items, summary.chunks as usize),
        (2, r.chunks_written)
    );
    let r = import(&mut p, &req).unwrap();
    assert_eq!((r.unchanged, r.chunks_written), (2, 0));
}

/// tests/fixtures/sample.xlsx (written by make_sample_xlsx.py): sheet "Ideas" has a blank first
/// row, an empty trailing header cell, a blank row, a text id with a leading zero, dates and decimals.
fn sample_xlsx() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.xlsx")
}

#[test]
fn excel_sheets_read_as_text_columns_and_import() {
    use invectus_ingest::source::{excel_sheets, read};
    let path = sample_xlsx();
    assert_eq!(excel_sheets(&path).unwrap(), vec!["Ideas", "Notes"]);

    let first = read(&Source::detect(&path).unwrap(), None).unwrap();
    assert_eq!(
        first.columns,
        vec!["id", "title", "abstract", "published", "score"],
        "an empty trailing header cell is not a column"
    );
    assert_eq!(first.rows.len(), 2, "blank rows are skipped");
    let r0 = &first.rows[0];
    assert_eq!(r0[0].as_deref(), Some("007"), "text ids keep leading zeros");
    assert_eq!(r0[2].as_deref(), Some("Text, with a comma"));
    assert_eq!(
        r0[3].as_deref(),
        Some("2025-01-01"),
        "date cells become ISO dates"
    );
    assert_eq!(r0[4].as_deref(), Some("3"), "whole numbers have no .0");
    let r1 = &first.rows[1];
    assert_eq!(r1[0].as_deref(), Some("8"));
    assert_eq!(r1[2].as_deref(), Some("Line one\nline two"));
    assert_eq!(r1[3].as_deref(), Some("2025-01-01T12:00:00"));
    assert_eq!(r1[4].as_deref(), Some("2.5"));

    let notes = Source::Excel {
        path: path.clone(),
        sheet: Some("Notes".into()),
    };
    let t = read(&notes, None).unwrap();
    assert_eq!(t.columns, vec!["name", "note"]);
    assert_eq!(t.rows[0][0].as_deref(), Some("Other sheet"));
    let missing = Source::Excel {
        path: path.clone(),
        sheet: Some("Nope".into()),
    };
    assert!(
        read(&missing, None)
            .err()
            .unwrap()
            .to_string()
            .contains("Ideas, Notes")
    );

    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create(&dir.path().join("p")).unwrap();
    let req = ImportRequest {
        collection: "ideas".into(),
        source: Source::detect(&path).unwrap(),
        mapping: Mapping {
            id: Some("id".into()),
            title: Some("title".into()),
            text: vec!["abstract".into()],
            date: Some("published".into()),
            url: None,
            role: RoleMapping::Constant {
                role: Role::Neutral,
            },
            meta: MetaMapping::All,
        },
        chunking: ChunkPolicy::default(),
        limit: None,
    };
    let r = import(&mut p, &req).unwrap();
    assert_eq!((r.inserted, r.skipped.len()), (2, 0));
    assert_eq!(
        import(&mut p, &req).unwrap().unchanged,
        2,
        "re-import is idempotent"
    );
    let stored: Source =
        serde_json::from_value(p.store.collection("ideas").unwrap().unwrap().source).unwrap();
    assert_eq!(
        stored, req.source,
        "the sheet choice is kept for re-imports"
    );
}
