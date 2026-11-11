//! Readers for table files (CSV, Parquet via DuckDB; Excel and OpenDocument via calamine),
//! folders of text files, and local snapshots of pulled sources (Google Sheets, Notion). Reading
//! never touches the network: a pulled source reads the snapshot [`crate::remote::pull`] wrote.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Source {
    Csv {
        path: PathBuf,
    },
    Parquet {
        path: PathBuf,
    },
    Folder {
        path: PathBuf,
        extensions: Vec<String>,
    },
    /// A spreadsheet workbook; `sheet` is the worksheet name (`None`: the first sheet).
    Excel {
        path: PathBuf,
        sheet: Option<String>,
    },
    /// A Google Sheet shared by link; read from its pulled CSV snapshot.
    GoogleSheet {
        spreadsheet_id: String,
        gid: Option<String>,
        snapshot: PathBuf,
    },
    /// A Notion database (one row per page); read from its pulled CSV snapshot.
    Notion {
        database_id: String,
        snapshot: PathBuf,
    },
    /// A Zotero library (`user` or `group:<id>`), optionally one collection; one row per item.
    Zotero {
        library: String,
        collection: Option<String>,
        snapshot: PathBuf,
    },
    /// An Airtable table (optionally through a view); one row per record.
    Airtable {
        base_id: String,
        table: String,
        view: Option<String>,
        snapshot: PathBuf,
    },
    /// A Typeform form's completed responses; one row per response.
    Typeform {
        form_id: String,
        snapshot: PathBuf,
    },
    /// A Tally form's completed submissions; one row per submission.
    Tally {
        form_id: String,
        snapshot: PathBuf,
    },
    /// Canny feature-request posts: one board (by id, URL name or name) or all boards.
    Canny {
        board: Option<String>,
        snapshot: PathBuf,
    },
    /// Productboard `notes` (customer feedback: demand) or `features` (the roadmap: coverage).
    Productboard {
        entity: String,
        snapshot: PathBuf,
    },
}

pub const EXCEL_EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];

pub const DEFAULT_FOLDER_EXTENSIONS: &[&str] = &["txt", "md", "markdown", "rst", "text"];
/// Files above this size are skipped (not plain notes; likely data dumps).
pub const MAX_FOLDER_FILE_BYTES: u64 = 5 * 1024 * 1024;

impl Source {
    /// Infer the kind from the path: a directory, `.csv`/`.tsv`, or `.parquet`.
    pub fn detect(path: &Path) -> Result<Self> {
        if path.is_dir() {
            return Ok(Source::Folder {
                path: path.to_path_buf(),
                extensions: DEFAULT_FOLDER_EXTENSIONS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            });
        }
        if !path.is_file() {
            bail!("{} does not exist", path.display());
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "csv" | "tsv" => Ok(Source::Csv {
                path: path.to_path_buf(),
            }),
            "parquet" | "pq" => Ok(Source::Parquet {
                path: path.to_path_buf(),
            }),
            e if EXCEL_EXTENSIONS.contains(&e) => Ok(Source::Excel {
                path: path.to_path_buf(),
                sheet: None,
            }),
            _ => bail!(
                "cannot infer the format of {}; expected a folder, .csv/.tsv, .parquet or a \
                 spreadsheet (.xlsx, .xls, .ods)",
                path.display()
            ),
        }
    }

    /// The local file or folder read on import (a pulled source's snapshot).
    pub fn path(&self) -> &Path {
        match self {
            Source::Csv { path }
            | Source::Parquet { path }
            | Source::Folder { path, .. }
            | Source::Excel { path, .. } => path,
            Source::GoogleSheet { snapshot, .. }
            | Source::Notion { snapshot, .. }
            | Source::Zotero { snapshot, .. }
            | Source::Airtable { snapshot, .. }
            | Source::Typeform { snapshot, .. }
            | Source::Tally { snapshot, .. }
            | Source::Canny { snapshot, .. }
            | Source::Productboard { snapshot, .. } => snapshot,
        }
    }

    /// A Google Sheets link or a Notion database (`notion:<id>` or a notion.so link) as a pulled
    /// source whose snapshot lives under `project_root/sources/`; `None` for anything else.
    pub fn parse_remote(spec: &str, project_root: &Path) -> Result<Option<Self>> {
        let spec = spec.trim();
        if let Some((id, gid)) = google_sheet_id(spec)? {
            let file = format!("{id}-{}.csv", gid.as_deref().unwrap_or("default"));
            return Ok(Some(Source::GoogleSheet {
                snapshot: project_root
                    .join("sources")
                    .join("google-sheets")
                    .join(file),
                spreadsheet_id: id,
                gid,
            }));
        }
        if let Some(id) = notion_database_id(spec)? {
            return Ok(Some(Source::Notion {
                snapshot: project_root
                    .join("sources")
                    .join("notion")
                    .join(format!("{id}.csv")),
                database_id: id,
            }));
        }
        if let Some((library, collection)) = zotero_library(spec)? {
            let file = format!(
                "{}-{}.csv",
                library.replace(':', "-"),
                collection.as_deref().unwrap_or("all")
            );
            return Ok(Some(Source::Zotero {
                snapshot: project_root.join("sources").join("zotero").join(file),
                library,
                collection,
            }));
        }
        if let Some((base_id, table, view)) = airtable_table(spec)? {
            let mut file = format!("{base_id}-{table}");
            if let Some(v) = &view {
                file.push_str(&format!("-{v}"));
            }
            return Ok(Some(Source::Airtable {
                snapshot: project_root
                    .join("sources")
                    .join("airtable")
                    .join(format!("{file}.csv")),
                base_id,
                table,
                view,
            }));
        }
        let snap = |kind: &str, name: &str| {
            project_root
                .join("sources")
                .join(kind)
                .join(format!("{name}.csv"))
        };
        if let Some(id) = form_id(
            spec,
            "typeform:",
            &[".typeform.com/to/", "admin.typeform.com/form/"],
        )? {
            return Ok(Some(Source::Typeform {
                snapshot: snap("typeform", &id),
                form_id: id,
            }));
        }
        if let Some(id) = form_id(spec, "tally:", &["tally.so/r/", "tally.so/forms/"])? {
            return Ok(Some(Source::Tally {
                snapshot: snap("tally", &id),
                form_id: id,
            }));
        }
        if let Some(rest) = spec.strip_prefix("canny:") {
            let board = match rest.trim() {
                "" | "all" => None,
                r => match r.strip_prefix("board:") {
                    Some(b) if !b.trim().is_empty() => Some(b.trim().to_string()),
                    _ => bail!("expected canny:all or canny:board:<id or name>, got {spec:?}"),
                },
            };
            let name = match &board {
                Some(b) => b
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() {
                            c.to_ascii_lowercase()
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>(),
                None => "all".into(),
            };
            return Ok(Some(Source::Canny {
                snapshot: snap("canny", &name),
                board,
            }));
        }
        if let Some(rest) = spec.strip_prefix("productboard:") {
            let entity = match rest.trim() {
                "notes" | "" => "notes",
                "features" => "features",
                _ => bail!("expected productboard:notes or productboard:features, got {spec:?}"),
            };
            return Ok(Some(Source::Productboard {
                snapshot: snap("productboard", entity),
                entity: entity.into(),
            }));
        }
        Ok(None)
    }

    /// The integration whose token a pull needs (`notion`, `zotero`, `airtable`, `typeform`,
    /// `tally`, `canny`).
    pub fn integration(&self) -> Option<&'static str> {
        match self {
            Source::Notion { .. } => Some("notion"),
            Source::Zotero { .. } => Some("zotero"),
            Source::Airtable { .. } => Some("airtable"),
            Source::Typeform { .. } => Some("typeform"),
            Source::Tally { .. } => Some("tally"),
            Source::Canny { .. } => Some("canny"),
            Source::Productboard { .. } => Some("productboard"),
            _ => None,
        }
    }

    /// A pulled source (needs a network pull before it can be read).
    pub fn is_remote(&self) -> bool {
        matches!(
            self,
            Source::GoogleSheet { .. }
                | Source::Notion { .. }
                | Source::Zotero { .. }
                | Source::Airtable { .. }
                | Source::Typeform { .. }
                | Source::Tally { .. }
                | Source::Canny { .. }
                | Source::Productboard { .. }
        )
    }

    /// The spec that names a pulled source again (for "pull and update").
    pub fn remote_spec(&self) -> Option<String> {
        match self {
            Source::GoogleSheet {
                spreadsheet_id,
                gid,
                ..
            } => Some(format!(
                "https://docs.google.com/spreadsheets/d/{spreadsheet_id}/edit{}",
                gid.as_deref()
                    .map(|g| format!("#gid={g}"))
                    .unwrap_or_default()
            )),
            Source::Notion { database_id, .. } => Some(format!("notion:{database_id}")),
            Source::Zotero {
                library,
                collection,
                ..
            } => Some(match (library.as_str(), collection) {
                ("user", None) => "zotero:library".to_string(),
                ("user", Some(c)) => format!("zotero:collection:{c}"),
                (lib, None) => format!("zotero:{lib}"),
                (lib, Some(c)) => format!("zotero:{lib}/collection:{c}"),
            }),
            Source::Airtable {
                base_id,
                table,
                view,
                ..
            } => Some(match view {
                Some(v) => format!("airtable:{base_id}/{table}/{v}"),
                None => format!("airtable:{base_id}/{table}"),
            }),
            Source::Typeform { form_id, .. } => Some(format!("typeform:{form_id}")),
            Source::Tally { form_id, .. } => Some(format!("tally:{form_id}")),
            Source::Canny { board, .. } => Some(match board {
                Some(b) => format!("canny:board:{b}"),
                None => "canny:all".into(),
            }),
            Source::Productboard { entity, .. } => Some(format!("productboard:{entity}")),
            _ => None,
        }
    }

    /// Short kind name for the UI and agents.
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Csv { .. } => "csv",
            Source::Parquet { .. } => "parquet",
            Source::Folder { .. } => "folder",
            Source::Excel { .. } => "excel",
            Source::GoogleSheet { .. } => "google_sheet",
            Source::Notion { .. } => "notion",
            Source::Zotero { .. } => "zotero",
            Source::Airtable { .. } => "airtable",
            Source::Typeform { .. } => "typeform",
            Source::Tally { .. } => "tally",
            Source::Canny { .. } => "canny",
            Source::Productboard { .. } => "productboard",
        }
    }

    /// Where the data comes from, for messages ("Google Sheet 1AbC…", "report.xlsx").
    pub fn describe(&self) -> String {
        match self {
            Source::GoogleSheet { spreadsheet_id, .. } => format!("Google Sheet {spreadsheet_id}"),
            Source::Notion { database_id, .. } => format!("Notion database {database_id}"),
            Source::Zotero {
                library,
                collection,
                ..
            } => {
                let lib = if library == "user" {
                    "your Zotero library".to_string()
                } else {
                    format!("Zotero {}", library.replace(':', " "))
                };
                match collection {
                    Some(c) => format!("{lib} (collection {c})"),
                    None => lib,
                }
            }
            Source::Airtable { base_id, table, .. } => format!("Airtable table {base_id}/{table}"),
            Source::Typeform { form_id, .. } => format!("Typeform form {form_id}"),
            Source::Tally { form_id, .. } => format!("Tally form {form_id}"),
            Source::Canny { board, .. } => match board {
                Some(b) => format!("Canny board {b}"),
                None => "all Canny boards".into(),
            },
            Source::Productboard { entity, .. } => format!("Productboard {entity}"),
            other => other.path().display().to_string(),
        }
    }
}

/// `(spreadsheet id, gid)` from a docs.google.com/spreadsheets link.
fn google_sheet_id(spec: &str) -> Result<Option<(String, Option<String>)>> {
    let rest = [
        "https://docs.google.com/spreadsheets/d/",
        "docs.google.com/spreadsheets/d/",
    ]
    .iter()
    .find_map(|p| spec.strip_prefix(p));
    let Some(rest) = rest else { return Ok(None) };
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if id.len() < 20 {
        bail!("this Google Sheets link has no spreadsheet id: {spec}");
    }
    let gid = spec
        .split(['#', '?', '&'])
        .find_map(|part| part.strip_prefix("gid="))
        .map(|g| {
            g.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|g| !g.is_empty());
    Ok(Some((id, gid)))
}

/// `(library, collection)` from `zotero:library`, `zotero:collection:<KEY>`,
/// `zotero:group:<id>[/collection:<KEY>]`, or a zotero.org library or collection link.
fn zotero_library(spec: &str) -> Result<Option<(String, Option<String>)>> {
    let key_ok = |k: &str| k.len() == 8 && k.chars().all(|c| c.is_ascii_alphanumeric());
    let mut library = "user".to_string();
    let mut collection = None;
    if let Some(rest) = spec.strip_prefix("zotero:") {
        for part in rest.split('/').filter(|p| !p.is_empty() && *p != "library") {
            if let Some(id) = part.strip_prefix("group:") {
                if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                    bail!("a Zotero group id is a number: {spec:?}");
                }
                library = format!("group:{id}");
            } else if let Some(k) = part.strip_prefix("collection:") {
                if !key_ok(k) {
                    bail!("a Zotero collection key has 8 letters or digits: {spec:?}");
                }
                collection = Some(k.to_uppercase());
            } else {
                bail!(
                    "expected zotero:library, zotero:collection:<KEY> or zotero:group:<id>, got \
                     {spec:?}"
                );
            }
        }
        return Ok(Some((library, collection)));
    }
    let lower = spec.to_ascii_lowercase();
    let Some(path) = [
        "https://www.zotero.org/",
        "https://zotero.org/",
        "zotero.org/",
    ]
    .iter()
    .find_map(|p| lower.strip_prefix(p)) else {
        return Ok(None);
    };
    let parts: Vec<&str> = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .split('/')
        .collect();
    if parts.first() == Some(&"groups") {
        let id = parts.get(1).copied().unwrap_or("");
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            bail!("this Zotero group link has no group id: {spec}");
        }
        library = format!("group:{id}");
    }
    if let Some(i) = parts.iter().position(|p| *p == "collections") {
        let k = parts.get(i + 1).copied().unwrap_or("");
        if !key_ok(k) {
            bail!("this Zotero link has no collection key: {spec}");
        }
        collection = Some(k.to_uppercase());
    }
    Ok(Some((library, collection)))
}

/// A form id from `<prefix><id>` or a link containing one of `markers` followed by the id.
fn form_id(spec: &str, prefix: &str, markers: &[&str]) -> Result<Option<String>> {
    let raw = if let Some(r) = spec.strip_prefix(prefix) {
        r
    } else if let Some(r) = markers
        .iter()
        .find_map(|m| spec.find(m).map(|i| &spec[i + m.len()..]))
    {
        r
    } else {
        return Ok(None);
    };
    let id: String = raw
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if id.len() < 4 {
        bail!("no form id in {spec:?}; copy the form's share or edit link");
    }
    Ok(Some(id))
}

/// `(base, table, view)` from an airtable.com link or `airtable:<app…>/<tbl…>[/<viw…>]`.
fn airtable_table(spec: &str) -> Result<Option<(String, String, Option<String>)>> {
    let rest = if let Some(r) = spec.strip_prefix("airtable:") {
        r
    } else if let Some(r) = ["https://airtable.com/", "airtable.com/"]
        .iter()
        .find_map(|p| spec.strip_prefix(p))
    {
        r.split(['?', '#']).next().unwrap_or("")
    } else {
        return Ok(None);
    };
    let id = |prefix: &str| {
        rest.split('/')
            .find(|p| p.starts_with(prefix) && p.len() == 17)
            .map(String::from)
    };
    match (id("app"), id("tbl")) {
        (Some(base), Some(table)) => Ok(Some((base, table, id("viw")))),
        _ => bail!(
            "copy the link of an Airtable table (it holds an app… base id and a tbl… table id): \
             {spec}"
        ),
    }
}

/// A Notion database id (32 hex digits, no dashes) from `notion:<id>` or a Notion link.
fn notion_database_id(spec: &str) -> Result<Option<String>> {
    let lower = spec.to_ascii_lowercase();
    let body = if let Some(rest) = lower.strip_prefix("notion:") {
        rest.to_string()
    } else if lower.contains("notion.so/") || lower.contains("notion.site/") {
        // The id is the last 32-hex run in the path, before any query (`?v=` names a view).
        lower.split('?').next().unwrap_or("").to_string()
    } else {
        return Ok(None);
    };
    let hex: Vec<char> = body.chars().filter(|c| *c != '-').collect();
    let id = hex
        .windows(32)
        .rev()
        .find(|w| w.iter().all(|c| c.is_ascii_hexdigit()))
        .map(|w| w.iter().collect::<String>());
    match id {
        Some(id) => Ok(Some(id)),
        None => bail!("no Notion database id in {spec:?}; copy the database's link from Notion"),
    }
}

/// A source read into named string columns. Folder sources expose fixed columns
/// (`path`, `title`, `text`, `bytes`, `modified`).
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Entries that could not be read, with the reason (folders only).
    pub unreadable: Vec<(String, String)>,
}

impl Table {
    pub fn column_index(&self, name: &str) -> Result<usize> {
        self.columns
            .iter()
            .position(|c| c == name)
            .with_context(|| {
                format!(
                    "column {name:?} not found; available columns: {}",
                    self.columns.join(", ")
                )
            })
    }
}

pub fn read(source: &Source, limit: Option<usize>) -> Result<Table> {
    match source {
        Source::Csv { path } => read_table(path, false, limit),
        Source::Parquet { path } => read_table(path, true, limit),
        Source::Folder { path, extensions } => read_folder(path, extensions, limit),
        Source::Excel { path, sheet } => read_excel(path, sheet.as_deref(), limit),
        Source::GoogleSheet { snapshot, .. }
        | Source::Notion { snapshot, .. }
        | Source::Zotero { snapshot, .. }
        | Source::Airtable { snapshot, .. }
        | Source::Typeform { snapshot, .. }
        | Source::Tally { snapshot, .. }
        | Source::Canny { snapshot, .. }
        | Source::Productboard { snapshot, .. } => {
            if !snapshot.is_file() {
                bail!(
                    "{} has not been pulled into this project yet; pull it first (Pull in the \
                     import screen, or `invectus pull`)",
                    source.describe()
                );
            }
            read_table(snapshot, false, limit)
        }
    }
}

/// Worksheet names of a workbook, in order.
pub fn excel_sheets(path: &Path) -> Result<Vec<String>> {
    use calamine::Reader;
    let wb = calamine::open_workbook_auto(path)
        .with_context(|| format!("opening {}", path.display()))?;
    Ok(wb.sheet_names().to_vec())
}

/// One worksheet as text columns. The first non-empty row is the header; blank header cells
/// become `column_N` and repeated names get a `_2`, `_3` suffix. Fully empty rows are skipped.
fn read_excel(path: &Path, sheet: Option<&str>, limit: Option<usize>) -> Result<Table> {
    use calamine::Reader;
    let mut wb = calamine::open_workbook_auto(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let names = wb.sheet_names().to_vec();
    let name = match sheet {
        Some(s) => names
            .iter()
            .find(|n| n.as_str() == s)
            .cloned()
            .with_context(|| {
                format!(
                    "{} has no sheet {s:?}; sheets: {}",
                    path.display(),
                    names.join(", ")
                )
            })?,
        None => names
            .first()
            .cloned()
            .with_context(|| format!("{} has no sheets", path.display()))?,
    };
    let range = wb
        .worksheet_range(&name)
        .with_context(|| format!("reading sheet {name:?} of {}", path.display()))?;
    let mut rows = range
        .rows()
        .map(|r| r.iter().map(cell_text).collect::<Vec<_>>())
        .filter(|r| r.iter().any(Option::is_some));
    let header = rows
        .next()
        .with_context(|| format!("sheet {name:?} of {} is empty", path.display()))?;
    let columns = header_names(&header);
    let n = columns.len();
    let rows = rows
        .take(limit.unwrap_or(usize::MAX))
        .map(|mut r| {
            r.resize(n, None);
            r
        })
        .collect();
    Ok(Table {
        columns,
        rows,
        unreadable: Vec::new(),
    })
}

fn header_names(header: &[Option<String>]) -> Vec<String> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    header
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let base = h.clone().unwrap_or_else(|| format!("column_{}", i + 1));
            let k = seen.entry(base.clone()).or_insert(0);
            *k += 1;
            if *k == 1 { base } else { format!("{base}_{k}") }
        })
        .collect()
}

/// A cell as text: whole numbers without `.0`, dates as ISO 8601, blanks and errors as `None`.
fn cell_text(c: &calamine::Data) -> Option<String> {
    use calamine::Data;
    let s = match c {
        Data::Empty | Data::Error(_) => return None,
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) if d.is_duration() => d.as_f64().to_string(),
        Data::DateTime(d) => excel_serial_iso(d.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
    };
    Some(s).filter(|s| !s.is_empty())
}

/// An Excel serial date (days since 1899-12-30) as `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS`.
fn excel_serial_iso(serial: f64) -> String {
    let days = serial.floor() as i64;
    let secs = ((serial - serial.floor()) * 86_400.0).round() as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days - 25_569 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    if secs == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        let secs = secs.min(86_399);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        )
    }
}

fn sql_string(path: &Path) -> Result<String> {
    let s = path
        .canonicalize()
        .with_context(|| format!("{} does not exist", path.display()))?
        .to_str()
        .context("path is not valid UTF-8")?
        .replace('\'', "''");
    Ok(format!("'{s}'"))
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn read_table(path: &Path, parquet: bool, limit: Option<usize>) -> Result<Table> {
    let conn = invectus_core::store::scratch_connection()?;
    let from = if parquet {
        format!("read_parquet({})", sql_string(path)?)
    } else {
        // all_varchar keeps ids such as PMIDs or zero-padded codes exactly as written.
        format!(
            "read_csv({}, header = true, all_varchar = true)",
            sql_string(path)?
        )
    };
    let mut describe = conn.prepare(&format!("DESCRIBE SELECT * FROM {from}"))?;
    let columns: Vec<String> = describe
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("reading {}", path.display()))?;
    if columns.is_empty() {
        bail!("{} has no columns", path.display());
    }
    let select = columns
        .iter()
        .map(|c| format!("CAST({} AS VARCHAR)", quote_ident(c)))
        .collect::<Vec<_>>()
        .join(", ");
    let limit = limit.map(|n| format!(" LIMIT {n}")).unwrap_or_default();
    let mut st = conn.prepare(&format!("SELECT {select} FROM {from}{limit}"))?;
    let n = columns.len();
    let rows = st
        .query_map([], |r| {
            (0..n).map(|i| r.get::<_, Option<String>>(i)).collect()
        })?
        .collect::<std::result::Result<Vec<Vec<Option<String>>>, _>>()
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(Table {
        columns,
        rows,
        unreadable: Vec::new(),
    })
}

fn read_folder(root: &Path, extensions: &[String], limit: Option<usize>) -> Result<Table> {
    if !root.is_dir() {
        bail!("{} is not a folder", root.display());
    }
    let mut files = Vec::new();
    let mut unreadable = Vec::new();
    walk(root, root, extensions, &mut files, &mut unreadable)?;
    files.sort();
    if let Some(n) = limit {
        files.truncate(n);
    }
    let mut rows = Vec::new();
    for rel in files {
        let full = root.join(&rel);
        let meta = std::fs::metadata(&full)?;
        let rel_s = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if meta.len() > MAX_FOLDER_FILE_BYTES {
            unreadable.push((rel_s, format!("larger than {MAX_FOLDER_FILE_BYTES} bytes")));
            continue;
        }
        let text = match String::from_utf8(std::fs::read(&full)?) {
            Ok(t) => t,
            Err(_) => {
                unreadable.push((rel_s, "not UTF-8 text".into()));
                continue;
            }
        };
        let title = markdown_title(&text).unwrap_or_else(|| {
            rel.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string());
        rows.push(vec![
            Some(rel_s),
            Some(title),
            Some(text),
            Some(meta.len().to_string()),
            modified,
        ]);
    }
    Ok(Table {
        columns: ["path", "title", "text", "bytes", "modified"]
            .map(String::from)
            .to_vec(),
        rows,
        unreadable,
    })
}

fn walk(
    root: &Path,
    dir: &Path,
    extensions: &[String],
    out: &mut Vec<PathBuf>,
    unreadable: &mut Vec<(String, String)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        // symlink_metadata: never follow links out of the chosen folder.
        let ft = std::fs::symlink_metadata(&path)?.file_type();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if ft.is_symlink() {
            unreadable.push((rel.to_string_lossy().into_owned(), "symbolic link".into()));
        } else if ft.is_dir() {
            walk(root, &path, extensions, out, unreadable)?;
        } else if ft.is_file() {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                out.push(rel);
            }
        }
    }
    Ok(())
}

/// First ATX heading (`# Title`) within the first lines of a Markdown file.
fn markdown_title(text: &str) -> Option<String> {
    text.lines()
        .take(20)
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_reader_skips_hidden_binary_and_other_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("sub")).unwrap();
        std::fs::create_dir_all(r.join(".git")).unwrap();
        std::fs::write(r.join("b.md"), "intro\n# Heading B\nbody").unwrap();
        std::fs::write(r.join("sub/a.txt"), "plain note").unwrap();
        std::fs::write(r.join(".hidden.md"), "secret").unwrap();
        std::fs::write(r.join(".git/x.md"), "ignored").unwrap();
        std::fs::write(r.join("img.png"), [0u8, 1, 2]).unwrap();
        std::fs::write(r.join("bad.txt"), [0xffu8, 0xfe, 0x00]).unwrap();
        let src = Source::detect(r).unwrap();
        let t = read(&src, None).unwrap();
        let paths: Vec<_> = t.rows.iter().map(|r| r[0].clone().unwrap()).collect();
        assert_eq!(paths, vec!["b.md", "sub/a.txt"]);
        assert_eq!(t.rows[0][1].as_deref(), Some("Heading B"));
        assert_eq!(
            t.rows[1][1].as_deref(),
            Some("a"),
            "txt falls back to file stem"
        );
        assert_eq!(
            t.unreadable,
            vec![("bad.txt".into(), "not UTF-8 text".into())]
        );
    }

    #[test]
    fn csv_columns_are_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.csv");
        std::fs::write(&p, "id,title,n\n007,\"A, quoted\",3\n008,B,\n").unwrap();
        let t = read(&Source::detect(&p).unwrap(), None).unwrap();
        assert_eq!(t.columns, vec!["id", "title", "n"]);
        assert_eq!(t.rows[0][0].as_deref(), Some("007"), "leading zeros kept");
        assert_eq!(t.rows[0][1].as_deref(), Some("A, quoted"));
        assert_eq!(t.rows[1][2], None);
        assert!(
            t.column_index("missing")
                .unwrap_err()
                .to_string()
                .contains("id, title, n")
        );
    }

    #[test]
    fn excel_serial_dates_become_iso() {
        assert_eq!(excel_serial_iso(45_658.0), "2025-01-01");
        assert_eq!(excel_serial_iso(45_658.5), "2025-01-01T12:00:00");
        assert_eq!(excel_serial_iso(1.0), "1899-12-31");
    }

    #[test]
    fn header_names_fill_blanks_and_dedupe() {
        let h = [Some("a".into()), None, Some("a".into())];
        assert_eq!(header_names(&h), vec!["a", "column_2", "a_2"]);
    }

    #[test]
    fn spreadsheet_extensions_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.xlsx");
        std::fs::write(&p, "x").unwrap();
        assert_eq!(
            Source::detect(&p).unwrap(),
            Source::Excel {
                path: p.clone(),
                sheet: None
            }
        );
        assert!(
            read(&Source::detect(&p).unwrap(), None).is_err(),
            "not a real workbook"
        );
    }

    #[test]
    fn remote_specs_parse_google_sheets_and_notion() {
        let root = Path::new("/p");
        let s = Source::parse_remote(
            "https://docs.google.com/spreadsheets/d/1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789/edit#gid=42",
            root,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            s,
            Source::GoogleSheet {
                spreadsheet_id: "1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789".into(),
                gid: Some("42".into()),
                snapshot: root
                    .join("sources/google-sheets/1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-42.csv"),
            }
        );
        let id = "0123456789abcdef0123456789abcdef";
        for spec in [
            format!("notion:{id}"),
            "notion:01234567-89ab-cdef-0123-456789abcdef".to_string(),
            format!(
                "https://www.notion.so/team/Reading-list-{id}?v=fedcba9876543210fedcba9876543210"
            ),
        ] {
            match Source::parse_remote(&spec, root).unwrap() {
                Some(Source::Notion { database_id, .. }) => assert_eq!(database_id, id, "{spec}"),
                other => panic!("{spec}: {other:?}"),
            }
        }
        let again = Source::parse_remote(&s.remote_spec().unwrap(), root)
            .unwrap()
            .unwrap();
        assert_eq!(again, s, "the spec round-trips");
        assert!(Source::parse_remote("notion:nothing", root).is_err());
        assert_eq!(Source::parse_remote("/data/x.csv", root).unwrap(), None);
    }

    #[test]
    fn remote_specs_parse_zotero_and_airtable() {
        let root = Path::new("/p");
        let zot = |spec: &str| match Source::parse_remote(spec, root).unwrap() {
            Some(Source::Zotero {
                library,
                collection,
                ..
            }) => (library, collection),
            other => panic!("{spec}: {other:?}"),
        };
        assert_eq!(zot("zotero:library"), ("user".into(), None));
        assert_eq!(
            zot("zotero:collection:ab12cd34"),
            ("user".into(), Some("AB12CD34".into()))
        );
        assert_eq!(
            zot("zotero:group:4711/collection:AB12CD34"),
            ("group:4711".into(), Some("AB12CD34".into()))
        );
        assert_eq!(
            zot("https://www.zotero.org/someone/collections/AB12CD34"),
            ("user".into(), Some("AB12CD34".into()))
        );
        assert_eq!(
            zot("https://www.zotero.org/groups/4711/lab/collections/AB12CD34/items"),
            ("group:4711".into(), Some("AB12CD34".into()))
        );
        assert!(Source::parse_remote("zotero:collection:short", root).is_err());

        let s = Source::parse_remote(
            "https://airtable.com/appAAAAAAAAAAAAAA/tblBBBBBBBBBBBBBB/viwCCCCCCCCCCCCCC?blocks=hide",
            root,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            s,
            Source::Airtable {
                base_id: "appAAAAAAAAAAAAAA".into(),
                table: "tblBBBBBBBBBBBBBB".into(),
                view: Some("viwCCCCCCCCCCCCCC".into()),
                snapshot: root.join(
                    "sources/airtable/appAAAAAAAAAAAAAA-tblBBBBBBBBBBBBBB-viwCCCCCCCCCCCCCC.csv"
                ),
            }
        );
        for spec in [
            s.remote_spec().unwrap(),
            Source::parse_remote("zotero:group:4711/collection:AB12CD34", root)
                .unwrap()
                .unwrap()
                .remote_spec()
                .unwrap(),
        ] {
            let again = Source::parse_remote(&spec, root).unwrap().unwrap();
            assert_eq!(again.remote_spec().unwrap(), spec, "the spec round-trips");
        }
        assert!(Source::parse_remote("https://airtable.com/appAAAAAAAAAAAAAA", root).is_err());
    }

    #[test]
    fn remote_specs_parse_forms_and_canny() {
        let root = Path::new("/p");
        let kind_id = |spec: &str| {
            let s = Source::parse_remote(spec, root).unwrap().unwrap();
            let again = Source::parse_remote(&s.remote_spec().unwrap(), root)
                .unwrap()
                .unwrap();
            assert_eq!(again, s, "{spec} round-trips");
            (s.kind(), s.remote_spec().unwrap())
        };
        assert_eq!(
            kind_id("https://acme.typeform.com/to/AbC123xy?utm=x"),
            ("typeform", "typeform:AbC123xy".into())
        );
        assert_eq!(
            kind_id("https://admin.typeform.com/form/AbC123xy/results"),
            ("typeform", "typeform:AbC123xy".into())
        );
        assert_eq!(
            kind_id("https://tally.so/r/w4Ex9z"),
            ("tally", "tally:w4Ex9z".into())
        );
        assert_eq!(kind_id("tally:w4Ex9z"), ("tally", "tally:w4Ex9z".into()));
        assert_eq!(kind_id("canny:all"), ("canny", "canny:all".into()));
        assert_eq!(
            kind_id("canny:board:Feature Requests"),
            ("canny", "canny:board:Feature Requests".into())
        );
        assert!(Source::parse_remote("canny:boards", root).is_err());
        assert_eq!(
            kind_id("productboard:features"),
            ("productboard", "productboard:features".into())
        );
        assert_eq!(
            kind_id("productboard:"),
            ("productboard", "productboard:notes".into())
        );
        assert!(Source::parse_remote("productboard:users", root).is_err());
        assert!(Source::parse_remote("tally:x", root).is_err());
    }

    #[test]
    fn unpulled_remote_source_says_to_pull() {
        let dir = tempfile::tempdir().unwrap();
        let s = Source::parse_remote("notion:0123456789abcdef0123456789abcdef", dir.path())
            .unwrap()
            .unwrap();
        let e = read(&s, None).err().unwrap().to_string();
        assert!(e.contains("not been pulled"), "{e}");
    }

    #[test]
    fn unknown_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.docx");
        std::fs::write(&p, "x").unwrap();
        assert!(Source::detect(&p).is_err());
    }
}
