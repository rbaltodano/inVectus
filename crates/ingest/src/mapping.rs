//! Schema mapping: source columns → item fields and role (SPEC §3, §4 step 1).

use crate::source::Table;
use anyhow::{Result, bail};
use invectus_core::Role;
use invectus_core::item::parse_date;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RoleMapping {
    /// Every item gets this role.
    Constant { role: Role },
    /// Look up a column. A rule matches when the value, or any element of a list value such as
    /// `[Journal Article, Systematic Review]` or `Journal Article; Systematic Review`, equals the
    /// rule's value (case-insensitive). The first matching rule wins; otherwise `default`.
    Column {
        column: String,
        rules: Vec<(String, Role)>,
        default: Role,
    },
}

impl Default for RoleMapping {
    fn default() -> Self {
        RoleMapping::Constant {
            role: Role::Neutral,
        }
    }
}

impl RoleMapping {
    /// Parse `"Systematic Review=coverage,Meta-Analysis=coverage"`.
    pub fn parse_rules(spec: &str) -> Result<Vec<(String, Role)>> {
        spec.split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|rule| {
                let Some((value, role)) = rule.rsplit_once('=') else {
                    bail!("role rule {rule:?} must look like VALUE=ROLE");
                };
                Ok((value.trim().to_string(), role.parse()?))
            })
            .collect()
    }

    fn resolve(&self, value: Option<&str>) -> Role {
        match self {
            RoleMapping::Constant { role } => *role,
            RoleMapping::Column { rules, default, .. } => {
                let Some(v) = value else { return *default };
                let v = v.trim();
                let elems: Vec<&str> = match v.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
                    Some(inner) => inner
                        .split(',')
                        .map(|s| s.trim().trim_matches('\''))
                        .collect(),
                    None => v.split(';').map(str::trim).collect(),
                };
                rules
                    .iter()
                    .find(|(rv, _)| elems.iter().any(|e| e.eq_ignore_ascii_case(rv)))
                    .map(|(_, r)| *r)
                    .unwrap_or(*default)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MetaMapping {
    /// Every column not mapped to a field.
    All,
    None,
    Columns {
        columns: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    /// Stable source identifier. Without one, rows are identified by position, which is not
    /// stable if the source is edited (import reports a warning).
    pub id: Option<String>,
    pub title: Option<String>,
    /// One or more columns joined with blank lines.
    pub text: Vec<String>,
    pub date: Option<String>,
    pub url: Option<String>,
    pub role: RoleMapping,
    pub meta: MetaMapping,
}

impl Mapping {
    /// Mapping for folder sources: fixed columns from the folder reader.
    pub fn folder(role: RoleMapping) -> Self {
        Mapping {
            id: Some("path".into()),
            title: Some("title".into()),
            text: vec!["text".into()],
            date: None,
            url: None,
            role,
            meta: MetaMapping::Columns {
                columns: vec!["path".into(), "bytes".into(), "modified".into()],
            },
        }
    }
}

/// A source row mapped to item fields, before ids and chunks are assigned.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MappedRow {
    pub source_id: String,
    pub title: Option<String>,
    pub text: String,
    pub url: Option<String>,
    pub date: Option<String>,
    /// The raw date value when it could not be parsed.
    pub unparsed_date: Option<String>,
    pub role: Role,
    pub meta: serde_json::Value,
}

/// Column positions resolved once per table.
pub struct Resolved<'a> {
    mapping: &'a Mapping,
    columns: &'a [String],
    id: Option<usize>,
    title: Option<usize>,
    text: Vec<usize>,
    date: Option<usize>,
    url: Option<usize>,
    role: Option<usize>,
    meta: Vec<usize>,
}

impl<'a> Resolved<'a> {
    pub fn new(mapping: &'a Mapping, table: &'a Table) -> Result<Self> {
        if mapping.text.is_empty() {
            bail!("map at least one text column");
        }
        let opt = |c: &Option<String>| c.as_deref().map(|c| table.column_index(c)).transpose();
        let text = mapping
            .text
            .iter()
            .map(|c| table.column_index(c))
            .collect::<Result<Vec<_>>>()?;
        let role = match &mapping.role {
            RoleMapping::Column { column, .. } => Some(table.column_index(column)?),
            RoleMapping::Constant { .. } => None,
        };
        let (id, title, date, url) = (
            opt(&mapping.id)?,
            opt(&mapping.title)?,
            opt(&mapping.date)?,
            opt(&mapping.url)?,
        );
        let mapped: Vec<usize> = [id, title, date, url, role]
            .into_iter()
            .flatten()
            .chain(text.iter().copied())
            .collect();
        let meta = match &mapping.meta {
            MetaMapping::All => (0..table.columns.len())
                .filter(|i| !mapped.contains(i))
                .collect(),
            MetaMapping::None => Vec::new(),
            MetaMapping::Columns { columns } => columns
                .iter()
                .map(|c| table.column_index(c))
                .collect::<Result<_>>()?,
        };
        Ok(Self {
            mapping,
            columns: &table.columns,
            id,
            title,
            text,
            date,
            url,
            role,
            meta,
        })
    }

    /// Map one row. Returns `None` when the row has no text.
    pub fn map(&self, row_number: usize, row: &[Option<String>]) -> Option<MappedRow> {
        let get = |i: Option<usize>| {
            i.and_then(|i| row[i].as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let text = self
            .text
            .iter()
            .filter_map(|&i| get(Some(i)))
            .collect::<Vec<_>>()
            .join("\n\n");
        if text.is_empty() {
            return None;
        }
        let raw_date = get(self.date);
        let date = raw_date.as_deref().and_then(parse_date);
        let meta = self
            .meta
            .iter()
            .filter_map(|&i| {
                row[i]
                    .as_ref()
                    .map(|v| (self.columns[i].clone(), v.clone().into()))
            })
            .collect::<serde_json::Map<_, _>>();
        Some(MappedRow {
            source_id: get(self.id).unwrap_or_else(|| format!("row:{row_number}")),
            title: get(self.title),
            text,
            url: get(self.url),
            unparsed_date: if date.is_none() { raw_date } else { None },
            date,
            role: self
                .mapping
                .role
                .resolve(self.role.and_then(|i| row[i].as_deref())),
            meta: serde_json::Value::Object(meta),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table {
            columns: ["pmid", "title", "abstract", "year", "types", "journal"]
                .map(String::from)
                .to_vec(),
            rows: vec![],
            unreadable: vec![],
        }
    }

    fn mapping() -> Mapping {
        Mapping {
            id: Some("pmid".into()),
            title: Some("title".into()),
            text: vec!["abstract".into()],
            date: Some("year".into()),
            url: None,
            role: RoleMapping::Column {
                column: "types".into(),
                rules: RoleMapping::parse_rules(
                    "Systematic Review=coverage,Journal Article=demand",
                )
                .unwrap(),
                default: Role::Neutral,
            },
            meta: MetaMapping::All,
        }
    }

    #[test]
    fn maps_fields_roles_dates_and_meta() {
        let t = table();
        let m = mapping();
        let r = Resolved::new(&m, &t).unwrap();
        let s = |v: &str| Some(v.to_string());
        let row = vec![
            s("42"),
            s(" Title "),
            s("Body"),
            s("2019"),
            s("[Journal Article, Systematic Review]"),
            s("Optom"),
        ];
        let out = r.map(0, &row).unwrap();
        assert_eq!(out.source_id, "42");
        assert_eq!(out.title.as_deref(), Some("Title"));
        assert_eq!(out.date.as_deref(), Some("2019-01-01"));
        assert_eq!(out.role, Role::Coverage, "first matching rule wins");
        assert_eq!(out.meta, serde_json::json!({"journal": "Optom"}));

        let mut row2 = row.clone();
        row2[4] = s("Editorial; Systematic Review");
        assert_eq!(
            r.map(1, &row2).unwrap().role,
            Role::Coverage,
            "';'-separated lists"
        );
        row2[4] = s("[Journal Article]");
        row2[3] = s("sometime");
        let out = r.map(1, &row2).unwrap();
        assert_eq!(out.role, Role::Demand);
        assert_eq!(out.date, None);
        assert_eq!(out.unparsed_date.as_deref(), Some("sometime"));

        let mut empty = row.clone();
        empty[2] = s("   ");
        assert!(r.map(2, &empty).is_none(), "rows without text are skipped");
    }

    #[test]
    fn missing_columns_are_reported() {
        let t = table();
        let mut m = mapping();
        m.text = vec!["body".into()];
        let err = Resolved::new(&m, &t).err().unwrap().to_string();
        assert!(
            err.contains("\"body\"") && err.contains("abstract"),
            "{err}"
        );
    }

    #[test]
    fn rule_parsing() {
        assert!(RoleMapping::parse_rules("x").is_err());
        assert!(RoleMapping::parse_rules("x=reviewish").is_err());
        assert_eq!(
            RoleMapping::parse_rules("a=b=coverage").unwrap(),
            vec![("a=b".to_string(), Role::Coverage)]
        );
    }
}
