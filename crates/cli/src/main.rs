//! `invectus`: the engine CLI over the same crates as the desktop app (SPEC §7.1).

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use invectus_core::store::RegionRecord;
use invectus_core::{CancelToken, Project, Role, TextEmbedder};
use invectus_embed::{
    EmbedOptions, ModelStore, ModelTokenizer, OnnxEmbedder, query_encoder, registry,
};
use invectus_eval::{BacktestOptions, backtest};
use invectus_gaps::{
    GapKind, ScanOptions, detail as gap_detail, list as gap_list, scan,
    set_verdict as set_gap_verdict,
};
use invectus_index::parity::run_parity;
use invectus_index::{
    EmbedJobOptions, SearchMode, SearchOptions, VectorStore, run_embed, search, select_table,
};
use invectus_ingest::gapmap::{GapmapPaths, import_gapmap, parse_models_yaml};
use invectus_ingest::{
    ChunkPolicy, ImportRequest, Mapping, MetaMapping, RoleMapping, Source, TokenChunking,
    import_with, preview,
};
use invectus_regions::{RegionOptions, STABLE_AT, map_data, run_regions};
use invectus_unsearch::{CalibrateOptions, CheckOptions, check, run_calibration};
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
#[allow(clippy::large_enum_variant)] // parsed once per run
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
    /// Fit what similarity means "the same thing" for a collection (needed by `check`).
    Calibrate(CalibrateArgs),
    /// Does an idea already exist in a collection? Calibrated verdict: exists, near or open.
    Check(CheckArgs),
    /// Regions (clusters of related items) and the 2-D map of a collection.
    Regions {
        #[command(subcommand)]
        command: RegionsCommand,
    },
    /// Gaps: coverage gaps, stale coverage and bridges between regions, with verdicts.
    Gaps {
        #[command(subcommand)]
        command: GapsCommand,
    },
    /// Describe the item that would sit between two regions (SPEC §6.3).
    Midpoint {
        #[command(flatten)]
        t: TableArgs,
        /// First fine region id.
        #[arg(long)]
        a: i32,
        /// Second fine region id.
        #[arg(long)]
        b: i32,
        #[command(flatten)]
        d: DescribeArgs,
    },
    /// Local language models (offline generation with a pinned GGUF model).
    Llm {
        #[command(subcommand)]
        command: LlmCommand,
    },
    /// Evaluation: backtest the gap ranking against what happened later (SPEC §9.1).
    Eval {
        #[command(subcommand)]
        command: EvalCommand,
    },
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
    /// Size chunks in this model's tokens instead of characters (its tokenizer must be
    /// installed), e.g. bge-small-en-v1.5.
    #[arg(long)]
    chunk_tokenizer: Option<String>,
    /// With --chunk-tokenizer: split texts longer than this many tokens.
    #[arg(long, default_value_t = TokenChunking::DEFAULT_MAX_TOKENS)]
    chunk_tokens: usize,
    /// With --chunk-tokenizer: tokens repeated at the start of the next chunk.
    #[arg(long, default_value_t = TokenChunking::DEFAULT_OVERLAP_TOKENS)]
    chunk_token_overlap: usize,
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
struct CalibrateArgs {
    #[command(flatten)]
    p: ProjectArg,
    #[arg(short, long)]
    collection: String,
    /// Vector space (id prefix or model key) when the collection has several.
    #[arg(long)]
    space: Option<String>,
    /// Items to sample (one same-thing pair and five related pairs each).
    #[arg(long, default_value_t = CalibrateOptions::default().sample)]
    sample: usize,
    /// Write the queries with this local language model (paraphrases of each item, SPEC §6.2)
    /// instead of using item titles; e.g. gemma-4-12b-it-qat-q4_0.
    #[arg(long, num_args = 0..=1, default_missing_value = invectus_llm::DEFAULT_LLM)]
    llm: Option<String>,
    #[arg(long, default_value_t = EmbedOptions::default().threads)]
    threads: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct CheckArgs {
    #[command(flatten)]
    p: ProjectArg,
    #[arg(short, long)]
    collection: String,
    /// The idea, in a sentence or two.
    query: String,
    #[arg(long)]
    space: Option<String>,
    /// Nearest items to show (at least 10 are compared).
    #[arg(short, default_value_t = 10)]
    k: usize,
    /// Compare only against items with this role (usually coverage).
    #[arg(long)]
    role: Option<Role>,
    /// Use the ANN index when there is one (faster, can miss a closer item).
    #[arg(long)]
    fast: bool,
    #[arg(long, default_value_t = 4)]
    threads: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct TableArgs {
    #[command(flatten)]
    p: ProjectArg,
    #[arg(short, long)]
    collection: String,
    /// Vector space (id prefix or model key) when the collection has several.
    #[arg(long)]
    space: Option<String>,
}

#[derive(Subcommand)]
enum RegionsCommand {
    /// Cluster the collection (replaces its previous regions and map).
    Run {
        #[command(flatten)]
        t: TableArgs,
        /// Neighbours per item in the similarity graph.
        #[arg(long, default_value_t = RegionOptions::default().k)]
        k: usize,
        /// Leiden resolution; higher gives more, smaller regions (default: automatic).
        #[arg(long, conflicts_with = "target_regions")]
        resolution: Option<f64>,
        /// Fine regions to aim for when the resolution is automatic (default: square root of
        /// the item count).
        #[arg(long)]
        target_regions: Option<usize>,
        /// Clustering runs with different seeds; regions are scored by how often they recur.
        #[arg(long, default_value_t = RegionOptions::default().seeds)]
        seeds: usize,
        /// Smaller communities become noise, attached to the nearest region.
        #[arg(long, default_value_t = RegionOptions::default().min_size)]
        min_size: usize,
        /// Number of broad regions (default: square root of the fine regions).
        #[arg(long)]
        broad: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// The latest regions with labels, sizes and stability.
    List {
        #[command(flatten)]
        t: TableArgs,
        #[arg(long)]
        json: bool,
    },
    /// Name regions with the local language model (resumes; skips named regions).
    Label {
        #[command(flatten)]
        t: TableArgs,
        #[arg(long, default_value = invectus_llm::DEFAULT_LLM)]
        model: String,
        /// Label at most this many regions in this run.
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = 8)]
        threads: usize,
        #[arg(long)]
        json: bool,
    },
    /// Write map data (regions and one point per item) as JSON.
    Map {
        #[command(flatten)]
        t: TableArgs,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum GapsCommand {
    /// Score gaps over the latest regions (replaces the previous scan; verdicts are kept).
    Scan {
        #[command(flatten)]
        t: TableArgs,
        /// Regions need at least this many demand items to be a coverage or stale gap.
        #[arg(long, default_value_t = ScanOptions::default().min_demand)]
        min_demand: usize,
        /// Bridges with more items between the two regions than this ratio are not gaps.
        #[arg(long, default_value_t = ScanOptions::default().max_bridge_ratio)]
        max_bridge_ratio: f64,
        #[arg(long)]
        json: bool,
    },
    /// Ranked gaps with their verdicts.
    List {
        #[command(flatten)]
        t: TableArgs,
        /// coverage, stale or bridge.
        #[arg(long)]
        kind: Option<GapKind>,
        /// Only gaps with this verdict ("none" for undecided).
        #[arg(long)]
        verdict: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// One gap: explanation, score terms, timeline and the items at its centre.
    Show {
        #[command(flatten)]
        t: TableArgs,
        id: i32,
        #[arg(long)]
        json: bool,
    },
    /// Describe what would fill a gap with the local language model, checked by a round trip.
    Describe {
        #[command(flatten)]
        t: TableArgs,
        id: i32,
        #[command(flatten)]
        d: DescribeArgs,
    },
    /// Check a gap against outside sources (PubMed, OpenAlex): keyword search, re-ranked
    /// locally against its description. Uses the network; results are cached in the project.
    Verify {
        #[command(flatten)]
        t: TableArgs,
        id: i32,
        /// Sources, comma-separated.
        #[arg(long, value_delimiter = ',', default_value = "pubmed,openalex")]
        sources: Vec<invectus_verify::Source>,
        /// Allow this command to use the network although the project's network switch is off.
        #[arg(long)]
        network: bool,
        /// Items fetched per source.
        #[arg(long, default_value_t = 20)]
        max: usize,
        /// Hits to show.
        #[arg(short, default_value_t = 10)]
        k: usize,
        #[arg(long)]
        json: bool,
    },
    /// Write a report of the ranked gaps (titles and links only, never source text).
    Export {
        #[command(flatten)]
        t: TableArgs,
        /// md or csv (from the file extension when omitted).
        #[arg(long)]
        format: Option<String>,
        /// Gaps in the Markdown report.
        #[arg(long, default_value_t = 25)]
        top: usize,
        #[arg(long)]
        out: PathBuf,
    },
    /// Record a decision: open, confirmed_gap, already_exists, not_viable or selected.
    Verdict {
        #[command(flatten)]
        t: TableArgs,
        id: i32,
        verdict: String,
        #[arg(long, default_value = "")]
        note: String,
    },
}

#[derive(Args)]
struct DescribeArgs {
    /// Position between two regions (bridges and `midpoint`): 0 = first region, 1 = second.
    #[arg(long, default_value_t = 0.5)]
    t: f32,
    /// What one item is, e.g. "research study" or "game".
    #[arg(long, default_value = "research study")]
    genre: String,
    /// What a coverage item is (coverage and stale gaps).
    #[arg(long, default_value = "systematic review")]
    coverage_genre: String,
    #[arg(long, default_value_t = 3)]
    tries: usize,
    #[arg(long, default_value = invectus_llm::DEFAULT_LLM)]
    model: String,
    #[arg(long, default_value_t = 8)]
    threads: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum LlmCommand {
    /// Pinned language models and whether they are installed.
    List,
    /// Re-hash the installed model file against its pinned SHA-256.
    Verify {
        #[arg(default_value = invectus_llm::DEFAULT_LLM)]
        key: String,
    },
    /// Generate a reply to a prompt (smoke test; offline).
    Generate {
        prompt: String,
        #[arg(long, default_value = invectus_llm::DEFAULT_LLM)]
        model: String,
        #[arg(long)]
        system: Option<String>,
        #[arg(long, default_value_t = 300)]
        max_tokens: usize,
        #[arg(long, default_value_t = 8)]
        threads: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum EvalCommand {
    /// Choose gap-score weights from a saved `eval backtest --json` file, leave-one-cutoff-out.
    Tune {
        /// Output of `invectus eval backtest --json`.
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Freeze the collection at each cutoff, rank gaps, and check which were filled later.
    Backtest {
        #[command(flatten)]
        t: TableArgs,
        /// Cutoff dates (YYYY-MM-DD), comma-separated.
        #[arg(long, value_delimiter = ',', required = true)]
        cutoff: Vec<String>,
        /// Years after each cutoff in which new coverage counts as filling a gap.
        #[arg(long, default_value_t = 2)]
        horizon_years: i32,
        #[arg(long)]
        json: bool,
    },
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
        Command::Import(a) => cmd_import(a, &cli.models_dir)?,
        Command::Embed(a) => cmd_embed(a, &cli.models_dir)?,
        Command::Search(a) => cmd_search(a, &cli.models_dir)?,
        Command::Calibrate(a) => cmd_calibrate(a, &cli.models_dir)?,
        Command::Check(a) => cmd_check(a, &cli.models_dir)?,
        Command::Regions { command } => cmd_regions(command, &cli.models_dir)?,
        Command::Gaps { command } => cmd_gaps(command, &cli.models_dir)?,
        Command::Llm { command } => cmd_llm(command, &cli.models_dir)?,
        Command::Midpoint { t, a, b, d } => {
            let target = invectus_midpoint::Target::Between { a, b, t: d.t };
            run_describe(&t, target, &d, &cli.models_dir)?
        }
        Command::Eval {
            command: EvalCommand::Tune { from, json },
        } => {
            let report: invectus_eval::BacktestReport = serde_json::from_slice(
                &std::fs::read(&from).with_context(|| format!("reading {}", from.display()))?,
            )?;
            let t = invectus_eval::tune(&report)?;
            if json {
                return print_json(&t);
            }
            let f = |r: &invectus_eval::RankScore| {
                format!(
                    "p@10 {:.3} p@25 {:.3} AUC {:.3}",
                    r.precision_at_10, r.precision_at_25, r.auc
                )
            };
            for fold in &t.folds {
                println!(
                    "held out {}: tuned {} | default {} | raw-count {} | train AUC {:.3} | weights {:?}",
                    fold.cutoff,
                    f(&fold.held_out),
                    f(&fold.default),
                    f(&fold.raw_count),
                    fold.train_auc,
                    fold.weights
                );
            }
            println!("mean held-out tuned:   {}", f(&t.mean_tuned));
            println!("mean held-out default: {}", f(&t.mean_default));
            println!("mean held-out raw:     {}", f(&t.mean_raw_count));
            println!(
                "weights on all cutoffs (not validated): {:?}",
                t.all_cutoffs_weights
            );
            for n in &t.notes {
                println!("note: {n}");
            }
        }
        Command::Eval {
            command:
                EvalCommand::Backtest {
                    t,
                    cutoff,
                    horizon_years,
                    json,
                },
        } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let vectors = VectorStore::open(&project)?;
            let r = backtest(
                &project,
                &vectors,
                &table,
                &BacktestOptions {
                    cutoffs: cutoff,
                    horizon_years,
                    regions: RegionOptions::default(),
                    scan: ScanOptions::default(),
                },
            )?;
            if json {
                return print_json(&r);
            }
            let f = |x: f64| {
                if x.is_nan() {
                    "-".to_string()
                } else {
                    format!("{x:.3}")
                }
            };
            println!(
                "{:<10} {:>6} {:>7} {:>5} {:>6} {:>6} | model p@10 p@25 AUC | raw-count p@10 p@25 AUC | density p@10 p@25 AUC",
                "cutoff", "items", "regions", "gaps", "filled", "base"
            );
            for c in &r.cutoffs {
                println!(
                    "{:<10} {:>6} {:>7} {:>5} {:>6} {:>6} | {} {} {} | {} {} {} | {} {} {}",
                    c.cutoff,
                    c.items,
                    c.regions,
                    c.gaps,
                    c.filled,
                    f(c.base_rate),
                    f(c.model.precision_at_10),
                    f(c.model.precision_at_25),
                    f(c.model.auc),
                    f(c.raw_count.precision_at_10),
                    f(c.raw_count.precision_at_25),
                    f(c.raw_count.auc),
                    f(c.density.precision_at_10),
                    f(c.density.precision_at_25),
                    f(c.density.auc)
                );
            }
            for (name, m) in [
                ("model", &r.mean_model),
                ("raw-count", &r.mean_raw_count),
                ("density", &r.mean_density),
            ] {
                if let Some(m) = m {
                    println!(
                        "mean {name}: p@10 {} p@25 {} AUC {}",
                        f(m.precision_at_10),
                        f(m.precision_at_25),
                        f(m.auc)
                    );
                }
            }
            for (name, m) in &r.mean_terms {
                println!(
                    "mean term alone ({name}): p@10 {} p@25 {} AUC {}",
                    f(m.precision_at_10),
                    f(m.precision_at_25),
                    f(m.auc)
                );
            }
            for n in &r.notes {
                println!("note: {n}");
            }
        }
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

fn cmd_import(a: ImportArgs, models_dir: &Option<PathBuf>) -> Result<()> {
    let source = Source::detect(&a.source)?;
    let tokenizer = a
        .chunk_tokenizer
        .as_deref()
        .map(|key| ModelTokenizer::load(&models(models_dir)?, key))
        .transpose()?;
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
        chunking: match &tokenizer {
            Some(t) => ChunkPolicy::tokens(t, a.chunk_tokens, a.chunk_token_overlap),
            None => ChunkPolicy {
                max_chars: a.chunk_chars,
                overlap_chars: a.chunk_overlap,
                tokens: None,
            },
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
    let r = import_with(
        &mut project,
        &req,
        tokenizer
            .as_ref()
            .map(|t| t as &dyn invectus_core::TokenOffsets),
    )?;
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

fn cmd_calibrate(a: CalibrateArgs, models_dir: &Option<PathBuf>) -> Result<()> {
    let project = open(&a.p)?;
    let table = select_table(&project, &a.collection, a.space.as_deref())?;
    let store = models(models_dir)?;
    let mut encoder = query_encoder(
        &store,
        &table.identity,
        &EmbedOptions {
            threads: a.threads,
            ..Default::default()
        },
    )?;
    let vectors = VectorStore::open(&project)?;
    let cancel = CancelToken::default();
    let c = cancel.clone();
    ctrlc::set_handler(move || {
        eprintln!("\ncancelling (nothing is saved until calibration finishes)…");
        c.cancel();
    })
    .context("installing Ctrl-C handler")?;
    let quiet = a.json;
    let mut last = Instant::now();
    let mut generator = a
        .llm
        .as_deref()
        .map(|key| {
            let spec = invectus_llm::registry::spec(key)?;
            invectus_llm::LlamaGenerator::load(&store.root, spec, a.threads)
        })
        .transpose()?;
    let r = run_calibration(
        &project,
        &vectors,
        &table,
        &mut encoder,
        generator
            .as_mut()
            .map(|g| g as &mut dyn invectus_llm::TextGenerator),
        &CalibrateOptions {
            sample: a.sample,
            ..Default::default()
        },
        &cancel,
        &mut |p| {
            if !quiet && (last.elapsed().as_secs_f64() > 2.0 || p.done == p.total) {
                last = Instant::now();
                eprintln!("  {}/{} steps", p.done, p.total);
            }
        },
    )?;
    if a.json {
        return print_json(&r);
    }
    if r.cancelled {
        println!("cancelled; nothing was saved");
        return Ok(());
    }
    println!(
        "calibrated {} · space {} ({}) · {}",
        r.collection,
        &r.space_id[..12],
        r.model_key,
        r.method
    );
    let spread = |s: &invectus_unsearch::calibrate::Spread| {
        format!("{:.3}/{:.3}/{:.3}", s.p10, s.p50, s.p90)
    };
    println!(
        "{} queries · similarity p10/p50/p90: own item {} · nearest other item {} · related items {}",
        r.items,
        spread(&r.own_text),
        spread(&r.nearest_other),
        spread(&r.related)
    );
    println!(
        "held out ({} queries): AUC {:.3}, balanced Brier {:.3}, balanced accuracy {:.3} · pairwise AUC (own vs related) {:.3}",
        r.holdout.items,
        r.holdout.auc,
        r.holdout.brier,
        r.holdout.balanced_accuracy,
        r.pairwise_auc
    );
    println!(
        "query → own item among all {} vectors: recall@1 {:.3}, recall@10 {:.3}, MRR {:.3}",
        r.vectors, r.known_item.recall_at_1, r.known_item.recall_at_10, r.known_item.mrr
    );
    let at = |s: Option<f32>| s.map_or("never".into(), |s| format!("{s:.3}"));
    println!(
        "P(exists) reaches 0.5 at nearest similarity {} and 0.7 at {} · embedding {:.1}s of {:.1}s",
        at(r.similarity_at_p50),
        at(r.similarity_at_p70),
        r.embed_seconds,
        r.seconds
    );
    for (title, idea) in r.examples.iter().take(5) {
        println!("example: {title}\n      → {idea}");
    }
    for n in &r.notes {
        println!("note: {n}");
    }
    Ok(())
}

fn cmd_check(a: CheckArgs, models_dir: &Option<PathBuf>) -> Result<()> {
    let project = open(&a.p)?;
    let table = select_table(&project, &a.collection, a.space.as_deref())?;
    // Fail on a missing calibration before spending time loading the model.
    invectus_unsearch::verdict::calibration_for(&project, &table)?;
    let store = models(models_dir)?;
    let mut encoder = query_encoder(
        &store,
        &table.identity,
        &EmbedOptions {
            threads: a.threads,
            batch_size: 1,
        },
    )?;
    let vectors = VectorStore::open(&project)?;
    let r = check(
        &project,
        &vectors,
        &table,
        &mut encoder,
        &a.query,
        &CheckOptions {
            k: a.k,
            role: a.role,
            mode: if a.fast {
                SearchMode::Auto
            } else {
                SearchMode::Exhaustive
            },
        },
    )?;
    if a.json {
        return print_json(&r);
    }
    println!(
        "{} · P(exists) {:.2} · {} ({}, space {})",
        r.verdict.as_str().to_uppercase(),
        r.p_exists,
        r.search.collection,
        r.search.model_key,
        &r.search.space_id[..12]
    );
    println!("{}", r.summary);
    if let Some(p) = &r.position {
        println!("{p}");
    }
    println!("next: {}", r.next_step);
    let f = |x: Option<f32>| x.map_or("-".into(), |x| format!("{x:.3}"));
    println!(
        "nearest similarity {} · drop to 10th {} · local density {} · {} · {} vectors compared",
        f(r.nearest_similarity),
        f(r.drop_to_10th),
        f(r.local_density),
        r.search.mode,
        r.search.vectors_compared
    );
    println!("calibration: {}", r.calibration.quality);
    for h in r.search.hits.iter().take(a.k) {
        println!(
            "{:>3}. {:.4}  [{}] {}{}",
            h.rank,
            h.similarity,
            h.item.role.as_str(),
            h.item.title.as_deref().unwrap_or(&h.item.source_id),
            h.item
                .date
                .as_deref()
                .map(|d| format!(" ({d})"))
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

fn print_regions(regions: &[RegionRecord]) {
    println!(
        "{:>5}  {:>6}  {:>9}  {:>5}  label",
        "fine", "items", "stability", "broad"
    );
    for r in regions.iter().filter(|r| r.level == "fine") {
        println!(
            "{:>5}  {:>6}  {:>9}  {:>5}  {}",
            r.region_id,
            r.size,
            r.stability.map_or("-".into(), |s| format!("{s:.2}")),
            r.parent_id.map_or("-".into(), |b| b.to_string()),
            match &r.llm_label {
                Some(l) => format!("{l}  [{}]", r.label),
                None => r.label.clone(),
            }
        );
    }
    for r in regions.iter().filter(|r| r.level == "broad") {
        println!("broad {} ({} items): {}", r.region_id, r.size, r.label);
    }
}

fn cmd_regions(c: RegionsCommand, models_dir: &Option<PathBuf>) -> Result<()> {
    match c {
        RegionsCommand::Run {
            t,
            k,
            resolution,
            target_regions,
            seeds,
            min_size,
            broad,
            json,
        } => {
            let mut project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let vectors = VectorStore::open(&project)?;
            let cancel = CancelToken::default();
            let c2 = cancel.clone();
            ctrlc::set_handler(move || {
                eprintln!("\ncancelling (previous regions are kept)…");
                c2.cancel();
            })
            .context("installing Ctrl-C handler")?;
            let r = run_regions(
                &mut project,
                &vectors,
                &table,
                &RegionOptions {
                    k,
                    resolution,
                    target_regions,
                    seeds,
                    min_size,
                    broad,
                    ..Default::default()
                },
                &cancel,
                &mut |p| {
                    if !json {
                        eprintln!("  {} ({}/{})", p.phase, p.done, p.total);
                    }
                },
            )?;
            if json {
                return print_json(&r);
            }
            if r.cancelled {
                println!("cancelled; previous regions are kept");
                return Ok(());
            }
            println!(
                "{}: {} items → {} fine regions in {} broad, {} noise items ({:.1}%) · sizes min/median/max {}/{}/{}",
                r.collection,
                r.items,
                r.fine,
                r.broad,
                r.noise,
                100.0 * r.noise as f64 / r.items as f64,
                r.sizes_min_median_max.0,
                r.sizes_min_median_max.1,
                r.sizes_min_median_max.2
            );
            println!(
                "resolution {:.2}{} · modularity {:.3} · stable regions (≥ {STABLE_AT}) {}/{} · mean stability {:.3} · map trustworthiness@15 {:.3}",
                r.resolution,
                r.target_regions
                    .map(|t| format!(" (automatic, target {t} regions)"))
                    .unwrap_or_default(),
                r.modularity,
                r.stable,
                r.fine,
                r.mean_stability,
                r.trustworthiness_at_15
            );
            println!(
                "kNN {} {:.1}s ({} edges{}), clustering {:.1}s, total {:.1}s",
                r.knn,
                r.seconds_knn,
                r.edges,
                r.knn_recall
                    .map(|x| format!(", recall@k {x:.3}"))
                    .unwrap_or_default(),
                r.seconds_cluster,
                r.seconds_total
            );
            print_regions(&r.regions);
        }
        RegionsCommand::List { t, json } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let data = map_data(&project, &table)?;
            if json {
                return print_json(&serde_json::json!({"run": data.run, "regions": data.regions}));
            }
            println!(
                "regions run {} ({})",
                data.run.run_id,
                data.run.created_at.as_deref().unwrap_or("")
            );
            print_regions(&data.regions);
        }
        RegionsCommand::Label {
            t,
            model,
            limit,
            threads,
            json,
        } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let spec = invectus_llm::registry::spec(&model)?;
            let mut generator =
                invectus_llm::LlamaGenerator::load(&models(models_dir)?.root, spec, threads)?;
            let cancel = CancelToken::default();
            let c2 = cancel.clone();
            ctrlc::set_handler(move || {
                eprintln!("\nstopping after this region (labels so far are kept)…");
                c2.cancel();
            })
            .context("installing Ctrl-C handler")?;
            let r = invectus_regions::llm_labels::label_regions(
                &project,
                &table,
                &mut generator,
                limit,
                &cancel,
                &mut |p| {
                    if !json {
                        eprintln!("  {}/{}", p.done, p.total);
                    }
                },
            )?;
            if json {
                return print_json(&r);
            }
            for (id, terms, name) in &r.labels {
                println!("{id:>5}  {name}   [{terms}]");
            }
            println!(
                "{} labelled, {} already labelled, {:.1}s ({:.2}s per region) with {}",
                r.labelled,
                r.already_labelled,
                r.seconds,
                r.seconds / r.labelled.max(1) as f64,
                r.model
            );
        }
        RegionsCommand::Map { t, out } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let data = map_data(&project, &table)?;
            std::fs::write(&out, serde_json::to_vec(&data)?)
                .with_context(|| format!("writing {}", out.display()))?;
            println!(
                "wrote {} regions and {} points to {}",
                data.regions.len(),
                data.points.len(),
                out.display()
            );
        }
    }
    Ok(())
}

fn print_gap(g: &invectus_gaps::GapView) {
    println!(
        "{:>3}. {:>6.2}  {:<8} {}  [{}]",
        g.gap.gap_id,
        g.gap.score,
        g.gap.kind.as_str(),
        g.gap.label,
        g.verdict
            .as_ref()
            .map_or("undecided", |v| v.verdict.as_str())
    );
    println!("       {}", g.gap.explanation);
}

fn run_describe(
    t: &TableArgs,
    target: invectus_midpoint::Target,
    d: &DescribeArgs,
    models_dir: &Option<PathBuf>,
) -> Result<()> {
    let project = open(&t.p)?;
    let table = select_table(&project, &t.collection, t.space.as_deref())?;
    let store = models(models_dir)?;
    let mut encoder = query_encoder(
        &store,
        &table.identity,
        &EmbedOptions {
            threads: 4,
            batch_size: 1,
        },
    )?;
    let spec = invectus_llm::registry::spec(&d.model)?;
    let mut generator = invectus_llm::LlamaGenerator::load(&store.root, spec, d.threads)?;
    let vectors = VectorStore::open(&project)?;
    let r = invectus_midpoint::describe(
        &project,
        &vectors,
        &table,
        &mut encoder,
        &mut generator,
        &target,
        &invectus_midpoint::DescribeOptions {
            genre: d.genre.clone(),
            coverage_genre: d.coverage_genre.clone(),
            tries: d.tries,
            ..Default::default()
        },
    )?;
    if d.json {
        return print_json(&r);
    }
    println!("{} · {:?}", r.label, r.target);
    for (i, a) in r.attempts.iter().enumerate() {
        let mark = if r.accepted == Some(i) {
            "ACCEPTED"
        } else {
            "rejected"
        };
        println!("\n[{}] {mark}: {}\n{}", i + 1, a.title, a.description);
        let o = &a.on_target;
        match (o.to_a, o.to_b) {
            (Some(sa), Some(sb)) => println!(
                "    on target: {} (to midpoint {:.3}, to A {:.3}, to B {:.3})",
                o.passed, o.to_target, sa, sb
            ),
            _ => println!(
                "    on target: {} (nearest region {:?} at {:.3})",
                o.passed,
                o.nearest_region,
                o.nearest_region_similarity.unwrap_or(f32::NAN)
            ),
        }
        if let Some(n) = &a.novelty {
            println!(
                "    novel: {} ({}, P(exists) {:.2}; nearest: {} {:.3})",
                n.passed,
                n.verdict,
                n.p_exists,
                n.nearest_title.as_deref().unwrap_or("-"),
                n.nearest_similarity.unwrap_or(f32::NAN)
            );
            if let Some((t, sim)) = &n.one_sided_nearest {
                println!("    not counted (reaches only one side): {t} {sim:.3}");
            }
        }
        println!("    {:.1}s", a.seconds);
    }
    for n in &r.notes {
        println!("note: {n}");
    }
    Ok(())
}

fn cmd_gaps(c: GapsCommand, models_dir: &Option<PathBuf>) -> Result<()> {
    match c {
        GapsCommand::Scan {
            t,
            min_demand,
            max_bridge_ratio,
            json,
        } => {
            let mut project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let vectors = VectorStore::open(&project)?;
            let r = scan(
                &mut project,
                &vectors,
                &table,
                &ScanOptions {
                    min_demand,
                    max_bridge_ratio,
                    ..Default::default()
                },
            )?;
            if json {
                return print_json(&r);
            }
            println!(
                "{}: {} coverage gaps, {} stale, {} bridges · {} unstable regions skipped · latest date {}",
                r.collection,
                r.coverage,
                r.stale,
                r.bridge,
                r.unstable_regions,
                r.latest_date.as_deref().unwrap_or("-")
            );
            for n in &r.notes {
                println!("note: {n}");
            }
            let l = gap_list(&project, &table)?;
            for g in l.gaps.iter().take(15) {
                print_gap(g);
            }
        }
        GapsCommand::List {
            t,
            kind,
            verdict,
            limit,
            json,
        } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let mut l = gap_list(&project, &table)?;
            l.gaps.retain(|g| {
                kind.is_none_or(|k| g.gap.kind == k)
                    && verdict.as_deref().is_none_or(|v| {
                        g.verdict.as_ref().map_or("none", |x| x.verdict.as_str()) == v
                    })
            });
            l.gaps.truncate(limit);
            if json {
                return print_json(&l);
            }
            for g in &l.gaps {
                print_gap(g);
            }
        }
        GapsCommand::Show { t, id, json } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let d = gap_detail(&project, &table, id)?;
            if json {
                return print_json(&d);
            }
            print_gap(&d.view);
            for term in &d.view.gap.terms {
                println!(
                    "       {:<45} value {:>8.3}  z {:>6.2}  × {:>5.2} = {:>6.2}",
                    term.name, term.value, term.z, term.weight, term.contribution
                );
            }
            if !d.view.gap.timeline.is_empty() {
                let years: Vec<String> = d
                    .view
                    .gap
                    .timeline
                    .iter()
                    .map(|y| format!("{} {}/{}", y.year, y.demand, y.coverage))
                    .collect();
                println!("       per year (demand/coverage): {}", years.join(", "));
            }
            if let Some(v) = &d.view.verdict {
                println!(
                    "       verdict {} ({}): {}",
                    v.verdict,
                    v.updated_at.as_deref().unwrap_or(""),
                    v.note
                );
            }
            println!("       items at the centre:");
            for i in &d.core_items {
                println!(
                    "         [{}] {}{}",
                    i.role,
                    i.title.as_deref().unwrap_or("(untitled)"),
                    i.date
                        .as_deref()
                        .map(|x| format!(" ({x})"))
                        .unwrap_or_default()
                );
            }
        }
        GapsCommand::Describe { t, id, d } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let g = gap_detail(&project, &table, id)?.view.gap;
            drop(project);
            let target = match g.region_b {
                Some(b) => invectus_midpoint::Target::Between {
                    a: g.region_a,
                    b,
                    t: d.t,
                },
                None => invectus_midpoint::Target::Region { region: g.region_a },
            };
            run_describe(&t, target, &d, models_dir)?;
        }
        GapsCommand::Verify {
            t,
            id,
            sources,
            network,
            max,
            k,
            json,
        } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let store = models(models_dir)?;
            let mut encoder = query_encoder(
                &store,
                &table.identity,
                &EmbedOptions {
                    threads: 4,
                    batch_size: 16,
                },
            )?;
            let v = invectus_verify::verify_gap(
                &project,
                &table,
                &mut encoder,
                id,
                &invectus_verify::VerifyOptions {
                    sources,
                    max_per_source: max,
                    allow_network: network,
                },
            )?;
            if json {
                return print_json(&v);
            }
            println!("gap {} ({}): {}", v.gap_id, v.kind, v.label);
            println!("{}", v.summary);
            for s in &v.sources {
                match &s.error {
                    Some(e) => println!("  {}: error: {e}", s.source),
                    None => println!(
                        "  {}: {} matching, {} fetched{} · {}",
                        s.source,
                        s.total.map_or("?".into(), |n| n.to_string()),
                        s.fetched,
                        if s.cached { " (cached)" } else { "" },
                        s.query
                    ),
                }
            }
            for h in v.hits.iter().take(k) {
                println!(
                    "{} {:.3}  [{}{}] {}",
                    if h.looks_like_match { "≈" } else { " " },
                    h.similarity,
                    h.hit.source,
                    h.hit.year.map(|y| format!(" {y}")).unwrap_or_default(),
                    h.hit.title
                );
                if let Some(u) = &h.hit.url {
                    println!("         {u}");
                }
            }
            for n in &v.notes {
                println!("note: {n}");
            }
        }
        GapsCommand::Export {
            t,
            format,
            top,
            out,
        } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let l = gap_list(&project, &table)?;
            let format = format.unwrap_or_else(|| {
                out.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            });
            let text = match format.as_str() {
                "csv" => invectus_gaps::export::csv(&l),
                "md" | "markdown" => {
                    invectus_gaps::export::markdown(&project, &t.collection, &l, top)?
                }
                other => bail!("unknown report format {other:?} (md or csv)"),
            };
            std::fs::write(&out, text).with_context(|| format!("writing {}", out.display()))?;
            println!("wrote {} gaps to {}", l.gaps.len(), out.display());
        }
        GapsCommand::Verdict {
            t,
            id,
            verdict,
            note,
        } => {
            let project = open(&t.p)?;
            let table = select_table(&project, &t.collection, t.space.as_deref())?;
            let g = set_gap_verdict(&project, &table, id, &verdict, &note)?;
            println!(
                "gap {id}: {verdict} (kept across re-runs while its core items stay together)"
            );
            print_gap(&g);
        }
    }
    Ok(())
}

fn cmd_llm(c: LlmCommand, models_dir: &Option<PathBuf>) -> Result<()> {
    let root = models(models_dir)?.root.clone();
    match c {
        LlmCommand::List => {
            println!("models folder: {}", root.display());
            for m in invectus_llm::MODELS {
                println!(
                    "{}{} — {}@{} · {} · {:.2} GB · {} · {}",
                    m.key,
                    if m.key == invectus_llm::DEFAULT_LLM {
                        " (default)"
                    } else {
                        ""
                    },
                    m.repo,
                    &m.revision[..12],
                    m.license,
                    m.bytes as f64 / 1e9,
                    if invectus_llm::store::is_installed(&root, m) {
                        "installed"
                    } else {
                        "not installed"
                    },
                    m.note
                );
            }
        }
        LlmCommand::Verify { key } => {
            let spec = invectus_llm::registry::spec(&key)?;
            let path = invectus_llm::store::model_path(&root, spec);
            let got = invectus_llm::store::sha256_file(&path)?;
            if got != spec.sha256 {
                bail!(
                    "{} has SHA-256 {got}, expected {}",
                    path.display(),
                    spec.sha256
                );
            }
            println!("{key}: {} matches its pinned SHA-256", path.display());
        }
        LlmCommand::Generate {
            prompt,
            model,
            system,
            max_tokens,
            threads,
            json,
        } => {
            use invectus_llm::TextGenerator;
            let spec = invectus_llm::registry::spec(&model)?;
            let t = Instant::now();
            let mut g = invectus_llm::LlamaGenerator::load(&root, spec, threads)?;
            let load_s = t.elapsed().as_secs_f64();
            let out = g.generate(
                system.as_deref(),
                &prompt,
                &invectus_llm::GenOptions {
                    max_tokens,
                    ..Default::default()
                },
            )?;
            if json {
                return print_json(
                    &serde_json::json!({"model": g.identity(), "load_seconds": load_s,
                    "generation": out}),
                );
            }
            println!("{}", out.text);
            eprintln!(
                "{} · load {:.1}s · {} prompt + {} generated tokens in {:.1}s ({:.1} tok/s){}",
                spec.key,
                load_s,
                out.prompt_tokens,
                out.generated_tokens,
                out.seconds,
                out.generated_tokens as f64 / out.seconds.max(1e-9),
                if out.truncated {
                    " · stopped at --max-tokens"
                } else {
                    ""
                }
            );
        }
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
