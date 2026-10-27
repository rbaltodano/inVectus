//! `invectus`: the engine CLI over the same crates as the desktop app (SPEC §7.1).

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use invectus_core::{CancelToken, Project, Role, TextEmbedder};
use invectus_embed::{EmbedOptions, ModelStore, OnnxEmbedder, query_encoder, registry};
use invectus_index::parity::run_parity;
use invectus_index::{
    EmbedJobOptions, SearchMode, SearchOptions, VectorStore, run_embed, search, select_table,
};
use invectus_ingest::gapmap::{GapmapPaths, import_gapmap, parse_models_yaml};
use invectus_ingest::{
    ChunkPolicy, ImportRequest, Mapping, MetaMapping, RoleMapping, Source, import, preview,
};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "invectus",
    version,
    about = "inVectus engine: import, embed and search collections"
)]
struct Cli {
    /// Refuse every network call (also: INVECTUS_OFFLINE=1). Import, embed and search never use
    /// the network; only `models download` does.
    #[arg(long, global = true)]
    offline: bool,
    /// Installed-models folder (default: OS app-data folder, or INVECTUS_MODELS_DIR).
    #[arg(long, global = true, env = "INVECTUS_MODELS_DIR")]
    models_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct ProjectArg {
    /// Project folder (`<name>.invectus`).
    #[arg(short, long, env = "INVECTUS_PROJECT")]
    project: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Create a project folder.
    Init { path: PathBuf },
    /// Collections, vector spaces and jobs of a project.
    Status {
        #[command(flatten)]
        p: ProjectArg,
        #[arg(long)]
        json: bool,
    },
    /// Import a CSV/TSV, Parquet file or a folder of text files into a collection.
    Import(ImportArgs),
    /// Embed a collection (resumable; re-running skips unchanged items).
    Embed(EmbedArgs),
    /// Nearest items to a query in a collection (positive search, not an absence verdict).
    Search(SearchArgs),
    /// Check that the local encoder reproduces a collection's imported vectors.
    Parity(ParityArgs),
    /// Manage embedding models.
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// Import from a gapmap project (read-only).
    Gapmap {
        #[command(subcommand)]
        command: GapmapCommand,
    },
}

#[derive(Args)]
struct ImportArgs {
    #[command(flatten)]
    p: ProjectArg,
    /// Source: a folder, .csv/.tsv or .parquet file.
    source: PathBuf,
    #[arg(short, long)]
    collection: String,
    /// Text column(s), comma-separated (tables only).
    #[arg(long, value_delimiter = ',')]
    text: Vec<String>,
    #[arg(long)]
    title: Option<String>,
    /// Stable id column (strongly recommended for re-imports).
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    date: Option<String>,
    #[arg(long)]
    url: Option<String>,
    /// Constant role for every item: demand, coverage or neutral.
    #[arg(long, conflicts_with = "role_column")]
    role: Option<Role>,
    /// Column that decides the role, with --role-rules.
    #[arg(long, requires = "role_rules")]
    role_column: Option<String>,
    /// VALUE=ROLE rules, e.g. "Systematic Review=coverage,Journal Article=demand".
    #[arg(long)]
    role_rules: Option<String>,
    #[arg(long, default_value = "neutral")]
    role_default: Role,
    /// Metadata columns: all (unmapped columns), none, or a comma-separated list.
    #[arg(long, default_value = "all")]
    meta: String,
    /// Split texts longer than this many characters (0 = never).
    #[arg(long, default_value_t = ChunkPolicy::default().max_chars)]
    chunk_chars: usize,
    #[arg(long, default_value_t = ChunkPolicy::default().overlap_chars)]
    chunk_overlap: usize,
    /// Read at most this many rows/files.
    #[arg(long)]
    limit: Option<usize>,
    /// Show the first N mapped items without writing anything.
    #[arg(long)]
    preview: Option<usize>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct EmbedArgs {
    #[command(flatten)]
    p: ProjectArg,
    #[arg(short, long)]
    collection: String,
    /// Registry model key. Default: the provisional CPU default from P0.
    #[arg(long, default_value = registry::DEFAULT_MODEL)]
    model: String,
    #[arg(long, default_value_t = EmbedOptions::default().batch_size)]
    batch: usize,
    #[arg(long, default_value_t = EmbedOptions::default().threads)]
    threads: usize,
    #[arg(long, default_value_t = EmbedJobOptions::default().flush_every)]
    flush_every: usize,
    /// Build an ANN index at or above this many vectors.
    #[arg(long, default_value_t = EmbedJobOptions::default().ann_min_rows)]
    ann_min_rows: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SearchArgs {
    #[command(flatten)]
    p: ProjectArg,
    #[arg(short, long)]
    collection: String,
    query: String,
    /// Vector space (id prefix or model key) when the collection has several.
    #[arg(long)]
    space: Option<String>,
    #[arg(short, default_value_t = 10)]
    k: usize,
    /// Only items with this role (e.g. coverage).
    #[arg(long)]
    role: Option<Role>,
    /// Exact similarity against every stored vector (the verification route).
    #[arg(long, conflicts_with = "nprobes")]
    exhaustive: bool,
    /// Force ANN with this many IVF partitions probed.
    #[arg(long)]
    nprobes: Option<usize>,
    #[arg(long, default_value_t = 4)]
    threads: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ParityArgs {
    #[command(flatten)]
    p: ProjectArg,
    #[arg(short, long)]
    collection: String,
    #[arg(long)]
    space: Option<String>,
    #[arg(long, default_value_t = 200)]
    sample: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum ModelsCommand {
    /// Pinned models and whether they are installed.
    List,
    /// Install from a local folder holding the pinned files (checksum-verified; hard-linked when
    /// possible). No network.
    Install {
        key: String,
        #[arg(long)]
        from: PathBuf,
    },
    /// Download the pinned files from Hugging Face (network; refused with --offline).
    Download { key: String },
    /// Re-hash installed files against their pinned checksums.
    Verify { key: String },
}

#[derive(Subcommand)]
enum GapmapCommand {
    /// Copy papers (roles from gapmap's synthesis/primary flags) and their Nomic vectors.
    Import {
        #[command(flatten)]
        p: ProjectArg,
        /// gapmap project root (containing data/ and config/).
        #[arg(long)]
        source: PathBuf,
        #[arg(short, long, default_value = "gapmap")]
        collection: String,
        /// Import at most this many vectors.
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn models(cli_dir: &Option<PathBuf>) -> Result<ModelStore> {
    Ok(ModelStore::new(match cli_dir {
        Some(d) => d.clone(),
        None => ModelStore::default_root()?,
    }))
}

fn open(p: &ProjectArg) -> Result<Project> {
    let project = Project::open(&p.project)?;
    if let Some(w) = project.storage_warning() {
        eprintln!("warning: {w}");
    }
    Ok(project)
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.offline {
        // SAFETY: set before any other thread is started.
        unsafe { std::env::set_var("INVECTUS_OFFLINE", "1") };
    }
    match cli.command {
        Command::Init { path } => {
            let p = Project::create(&path)?;
            println!("created {}", p.root.display());
            if let Some(w) = p.storage_warning() {
                eprintln!("warning: {w}");
            }
        }
        Command::Status { p, json } => status(&open(&p)?, json)?,
        Command::Import(a) => cmd_import(a)?,
        Command::Embed(a) => cmd_embed(a, &cli.models_dir)?,
        Command::Search(a) => cmd_search(a, &cli.models_dir)?,
        Command::Parity(a) => cmd_parity(a, &cli.models_dir)?,
        Command::Models { command } => cmd_models(command, &cli.models_dir)?,
        Command::Gapmap {
            command:
                GapmapCommand::Import {
                    p,
                    source,
                    collection,
                    limit,
                    json,
                },
        } => {
            let mut project = open(&p)?;
            let vectors = VectorStore::open(&project)?;
            let paths = GapmapPaths::from_root(&source)?;
            let model = parse_models_yaml(&std::fs::read_to_string(&paths.models_yaml)?, "nomic")?;
            let tokenizer = registry::spec_for_revision(&model.repo, &model.revision)
                .map(|s| s.file("tokenizer.json").sha256.to_string())
                .unwrap_or_else(|| format!("unrecorded:{}@{}", model.repo, model.revision));
            let t = Instant::now();
            let r = import_gapmap(
                &mut project,
                &vectors,
                &paths,
                &collection,
                limit,
                &tokenizer,
            )?;
            if json {
                return print_json(&r);
            }
            println!(
                "gapmap → {}: {} vectors read; items +{} ~{} ={}; vectors +{} ={}; {} without paper row ({:.1}s)",
                r.collection,
                r.vectors_read,
                r.items_inserted,
                r.items_updated,
                r.items_unchanged,
                r.vectors_added,
                r.vectors_unchanged,
                r.vectors_without_paper,
                t.elapsed().as_secs_f64()
            );
            println!("roles: {:?}", r.roles);
            println!(
                "space {} ({}, imported; searching requires `invectus parity`)",
                &r.space_id[..12],
                r.model_id
            );
        }
    }
    Ok(())
}

fn status(p: &Project, json: bool) -> Result<()> {
    let collections = p.store.collection_summaries()?;
    let tables = p.store.vector_tables(None)?;
    let jobs = p.store.jobs()?;
    if json {
        let vectors = VectorStore::open(p)?;
        let tables: Vec<_> = tables
            .iter()
            .map(|t| {
                serde_json::json!({"collection": t.collection_id, "space_id": t.space_id,
                    "table": t.table_name, "origin": t.origin, "identity": t.identity,
                    "ann": t.ann, "vectors": vectors.count(t).ok()})
            })
            .collect();
        return print_json(&serde_json::json!({"root": p.root, "config": p.config,
            "collections": collections, "vector_tables": tables, "jobs": jobs}));
    }
    println!("project {} ({})", p.config.name, p.root.display());
    println!(
        "network: {}",
        if p.config.network_enabled {
            "enabled"
        } else {
            "off"
        }
    );
    for c in &collections {
        println!(
            "collection {}: {} items, {} chunks, roles {:?}",
            c.collection_id, c.items, c.chunks, c.by_role
        );
    }
    if !tables.is_empty() {
        let vectors = VectorStore::open(p)?;
        for t in &tables {
            println!(
                "  vectors {} · space {} · {} ({}, {}d, {} pooling, {}) · {} · {} vectors{}",
                t.collection_id,
                &t.space_id[..12],
                t.identity.model_key,
                t.identity.runtime,
                t.identity.dimensions,
                serde_json::to_value(t.identity.pooling)?
                    .as_str()
                    .unwrap_or(""),
                t.identity.compute_precision,
                t.origin,
                vectors.count(t)?,
                t.ann
                    .as_ref()
                    .map(|a| format!(" · ANN {} partitions", a["partitions"]))
                    .unwrap_or_default()
            );
        }
    }
    for j in jobs.iter().take(10) {
        println!("job {}: {} ({:.0}%)", j.job_id, j.state, j.progress * 100.0);
    }
    Ok(())
}

fn cmd_import(a: ImportArgs) -> Result<()> {
    let source = Source::detect(&a.source)?;
    let role = match (&a.role, &a.role_column) {
        (Some(r), _) => RoleMapping::Constant { role: *r },
        (None, Some(col)) => RoleMapping::Column {
            column: col.clone(),
            rules: RoleMapping::parse_rules(a.role_rules.as_deref().unwrap_or(""))?,
            default: a.role_default,
        },
        (None, None) => RoleMapping::Constant {
            role: a.role_default,
        },
    };
    let mapping = match &source {
        Source::Folder { .. } => {
            if !a.text.is_empty() || a.title.is_some() || a.id.is_some() {
                bail!(
                    "folder sources have fixed columns (path, title, text); omit --text/--title/--id"
                );
            }
            Mapping::folder(role)
        }
        _ => {
            if a.text.is_empty() {
                bail!("--text is required for table sources");
            }
            Mapping {
                id: a.id,
                title: a.title,
                text: a.text,
                date: a.date,
                url: a.url,
                role,
                meta: match a.meta.as_str() {
                    "all" => MetaMapping::All,
                    "none" => MetaMapping::None,
                    list => MetaMapping::Columns {
                        columns: list.split(',').map(|s| s.trim().to_string()).collect(),
                    },
                },
            }
        }
    };
    let req = ImportRequest {
        collection: a.collection,
        source,
        mapping,
        chunking: ChunkPolicy {
            max_chars: a.chunk_chars,
            overlap_chars: a.chunk_overlap,
        },
        limit: a.limit,
    };
    if let Some(n) = a.preview {
        let pv = preview(&req, n)?;
        if a.json {
            return print_json(&pv);
        }
        println!("columns: {}", pv.columns.join(", "));
        for r in &pv.rows {
            let text: String = r.text.chars().take(100).collect();
            println!(
                "- [{}] {} | {} | {} | {}…",
                r.role.as_str(),
                r.source_id,
                r.title.as_deref().unwrap_or("(no title)"),
                r.date.as_deref().unwrap_or("-"),
                text.replace('\n', " ")
            );
        }
        if !pv.skipped.is_empty() {
            println!("skipped: {:?}", pv.skipped);
        }
        return Ok(());
    }
    let mut project = open(&a.p)?;
    let t = Instant::now();
    let r = import(&mut project, &req)?;
    if a.json {
        return print_json(&r);
    }
    println!(
        "{}: {} rows read → {} items (+{} new, {} updated, {} unchanged), {} chunked into {} chunks ({:.1}s)",
        r.collection,
        r.source_rows,
        r.items,
        r.inserted,
        r.updated,
        r.unchanged,
        r.items_chunked,
        r.chunks_written,
        t.elapsed().as_secs_f64()
    );
    if !r.skipped.is_empty() {
        println!("skipped: {:?}", r.skipped);
    }
    if r.not_in_source > 0 {
        println!(
            "{} existing item(s) were not in this source (kept)",
            r.not_in_source
        );
    }
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

fn cmd_embed(a: EmbedArgs, models_dir: &Option<PathBuf>) -> Result<()> {
    let project = open(&a.p)?;
    project.store.require_collection(&a.collection)?;
    let store = models(models_dir)?;
    let opts = EmbedOptions {
        threads: a.threads,
        batch_size: a.batch,
    };
    let spec = registry::spec(&a.model)?;
    if !a.json {
        eprintln!(
            "model {} ({}@{}), CPU, {} threads, batch {}",
            spec.key,
            spec.repo,
            &spec.revision[..12],
            opts.threads,
            opts.batch_size
        );
    }
    let mut embedder = OnnxEmbedder::load(&store, &a.model, &opts)?;
    let vectors = VectorStore::open(&project)?;
    let cancel = CancelToken::default();
    let c = cancel.clone();
    ctrlc::set_handler(move || {
        eprintln!("\ncancelling after the current batch (progress is kept; re-run to resume)…");
        c.cancel();
    })
    .context("installing Ctrl-C handler")?;
    let started = Instant::now();
    let mut last = Instant::now();
    let quiet = a.json;
    let r = run_embed(
        &project,
        &vectors,
        &a.collection,
        &mut embedder,
        &EmbedJobOptions {
            flush_every: a.flush_every,
            ann_min_rows: a.ann_min_rows,
        },
        &cancel,
        &mut |p| {
            if !quiet && (last.elapsed().as_secs_f64() > 2.0 || p.done == p.total) {
                last = Instant::now();
                let rate = p.done as f64 / started.elapsed().as_secs_f64().max(1e-9);
                eprintln!("  {}/{} embedded ({rate:.1}/s)", p.done, p.total);
            }
        },
    )?;
    if a.json {
        return print_json(&r);
    }
    println!(
        "{}: {} inputs, {} already embedded, {} embedded ({:.1}/s), {} stale removed, {} truncated at {} tokens, {} vectors in {}{}",
        a.collection,
        r.inputs,
        r.already_embedded,
        r.embedded,
        r.items_per_second,
        r.removed_stale,
        r.truncated,
        embedder.identity().max_tokens,
        r.vectors_in_table,
        r.table,
        r.ann_partitions
            .map(|p| format!(", ANN index {p} partitions"))
            .unwrap_or_default()
    );
    if r.cancelled {
        println!("cancelled; re-run the same command to resume");
    }
    Ok(())
}

fn cmd_search(a: SearchArgs, models_dir: &Option<PathBuf>) -> Result<()> {
    let project = open(&a.p)?;
    let table = select_table(&project, &a.collection, a.space.as_deref())?;
    let store = models(models_dir)?;
    let t = Instant::now();
    let mut encoder = query_encoder(
        &store,
        &table.identity,
        &EmbedOptions {
            threads: a.threads,
            batch_size: 1,
        },
    )?;
    let load_ms = t.elapsed().as_secs_f64() * 1e3;
    let vectors = VectorStore::open(&project)?;
    let mode = if a.exhaustive {
        SearchMode::Exhaustive
    } else if let Some(n) = a.nprobes {
        SearchMode::Ann { nprobes: Some(n) }
    } else {
        SearchMode::Auto
    };
    let r = search(
        &project,
        &vectors,
        &table,
        &mut encoder,
        &a.query,
        &SearchOptions {
            k: a.k,
            role: a.role,
            mode,
        },
    )?;
    if a.json {
        return print_json(&r);
    }
    println!(
        "{} nearest in {} · {} · space {} ({})",
        r.hits.len(),
        r.collection,
        r.mode,
        &r.space_id[..12],
        r.model_key
    );
    println!(
        "query embed {:.1} ms, retrieval {:.1} ms over {} vectors (model load {:.0} ms) · {}",
        r.embed_ms, r.retrieve_ms, r.vectors_compared, load_ms, r.compatibility
    );
    for h in &r.hits {
        println!(
            "{:>3}. {:.4}  [{}] {}{}{}",
            h.rank,
            h.similarity,
            h.item.role.as_str(),
            h.item.title.as_deref().unwrap_or(&h.item.source_id),
            h.item
                .date
                .as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default(),
            h.via_chunk
                .map(|c| format!(" · via chunk {c}"))
                .unwrap_or_default()
        );
        if let Some(u) = &h.item.url {
            println!("       {u}");
        }
    }
    for n in &r.notes {
        println!("note: {n}");
    }
    Ok(())
}

fn cmd_parity(a: ParityArgs, models_dir: &Option<PathBuf>) -> Result<()> {
    let project = open(&a.p)?;
    let table = select_table(&project, &a.collection, a.space.as_deref())?;
    let store = models(models_dir)?;
    let mut encoder =
        OnnxEmbedder::load_for_space(&store, &table.identity, &EmbedOptions::default())?;
    let vectors = VectorStore::open(&project)?;
    let t = Instant::now();
    let r = run_parity(&project, &vectors, &table, &mut encoder, a.sample)?;
    if a.json {
        return print_json(&r);
    }
    println!(
        "parity {} vs {}: n={} mean cos {:.5}, p05 {:.5}, min {:.5} → {} (thresholds mean ≥ {}, min ≥ {}; {:.1}s)",
        r.encoder_runtime,
        r.stored_runtime,
        r.record.sample,
        r.record.mean_cos,
        r.record.p05_cos,
        r.record.min_cos,
        if r.record.passed { "PASS" } else { "FAIL" },
        r.thresholds.0,
        r.thresholds.1,
        t.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cmd_models(c: ModelsCommand, models_dir: &Option<PathBuf>) -> Result<()> {
    let store = models(models_dir)?;
    match c {
        ModelsCommand::List => {
            println!("models folder: {}", store.root.display());
            for m in registry::MODELS {
                println!(
                    "{}{} — {}@{} · {}d · {} MB · {} · {}",
                    m.key,
                    if m.key == registry::DEFAULT_MODEL {
                        " (default)"
                    } else {
                        ""
                    },
                    m.repo,
                    &m.revision[..12],
                    m.dimensions,
                    m.total_bytes() / 1_000_000,
                    if store.is_installed(m) {
                        "installed"
                    } else {
                        "not installed"
                    },
                    m.note
                );
            }
        }
        ModelsCommand::Install { key, from } => {
            let spec = registry::spec(&key)?;
            let how = store.install_from_dir(spec, &from)?;
            println!(
                "installed {} into {} ({how:?}, checksums verified)",
                spec.key,
                store.dir(spec).display()
            );
        }
        ModelsCommand::Download { key } => {
            let spec = registry::spec(&key)?;
            eprintln!(
                "downloading {} ({} MB) from huggingface.co at pinned revision {}",
                spec.key,
                spec.total_bytes() / 1_000_000,
                spec.revision
            );
            store.download(spec, &mut |m| eprintln!("  {m}"))?;
            println!("installed {} into {}", spec.key, store.dir(spec).display());
        }
        ModelsCommand::Verify { key } => {
            let spec = registry::spec(&key)?;
            // Force a full re-hash by verifying each file directly.
            let dir = store.dir(spec);
            for f in spec.files {
                let got = invectus_embed::store::sha256_file(&dir.join(f.local))?;
                if got != f.sha256 {
                    bail!("{} has SHA-256 {got}, expected {}", f.local, f.sha256);
                }
            }
            store.verified_dir(spec)?;
            println!(
                "{}: all {} files match their pinned SHA-256",
                spec.key,
                spec.files.len()
            );
        }
    }
    Ok(())
}
