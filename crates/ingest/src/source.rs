//! Readers for table files (CSV, Parquet via DuckDB) and folders of text files.

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
}

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
            _ => bail!(
                "cannot infer the format of {}; expected a folder, .csv/.tsv or .parquet",
                path.display()
            ),
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Source::Csv { path } | Source::Parquet { path } | Source::Folder { path, .. } => path,
        }
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
    fn unknown_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.xlsx");
        std::fs::write(&p, "x").unwrap();
        assert!(Source::detect(&p).is_err());
    }
}
