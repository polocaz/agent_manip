//! Read-only access to the agent database (collect.sqlite3), shared by the
//! CLI and the web dashboard — including SysTrack string-ID resolution.
//!
//! Schema fact (verified against 11.6 endpoint snapshots): inventory and
//! execution tables store integer string-IDs, not names. `SASTR (STRINGID,
//! STRVALUE)` holds system-scope strings; `SASTRUSER` holds user-scope
//! strings (account names, per-user scan strings). Columns like
//! `SASFW.PACKAGE/PUB/DISPLAYVER`, `SAAPP.APP_ID/PATH_ID/CMDLINE_ID` and
//! `SASFWUSER.ACCOUNT_ID` all join `= STRINGID`. A raw ID dump is useless to
//! a human, so everything that renders table rows should resolve them.
//!
//! All queries shell out to `sqlite3 -readonly` so we can never corrupt a
//! live agent database.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::paths;

/// Per-column map of string-ID → resolved string value, for the columns of a
/// row set that turned out to be SASTR/SASTRUSER references.
pub type ResolvedStrings = BTreeMap<String, BTreeMap<i64, String>>;

/// Which string table a column's IDs live in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrScope {
    /// System-scope strings: `SASTR`.
    System,
    /// User-scope strings (account names etc.): `SASTRUSER`.
    User,
}

/// Run read-only SQL against the live agent DB, returning sqlite3's -json
/// output (an empty result set prints nothing — normalized to `[]`).
pub fn query_json(sql: &str) -> Result<Value> {
    let db = paths::database_path();
    if !db.exists() {
        bail!("database not found at {}", db.display());
    }
    let out = Command::new("sqlite3")
        .arg("-readonly")
        .arg("-json")
        .arg(&db)
        .arg(sql)
        .output()
        .map_err(|e| anyhow!("running sqlite3: {}", e))?;
    if !out.status.success() {
        let hint = if unsafe { libc::geteuid() } != 0 {
            " — the database is root-owned; run lsman with sudo"
        } else {
            ""
        };
        bail!(
            "sqlite3: {}{}",
            String::from_utf8_lossy(&out.stderr).trim(),
            hint
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    serde_json::from_str(trimmed).map_err(|e| anyhow!("parsing sqlite3 -json output: {}", e))
}

/// All user table names, sorted.
pub fn table_names() -> Result<Vec<String>> {
    let rows = query_json(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name;",
    )?;
    Ok(rows
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r.get("name")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// `{name, n}` rows: every user table with its row count, in one pass.
pub fn table_counts() -> Result<Value> {
    let names = table_names()?;
    if names.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let sql = names
        .iter()
        .map(|n| {
            format!(
                "SELECT '{}' AS name, count(*) AS n FROM \"{}\"",
                escape_str(n),
                escape_ident(n)
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ")
        + ";";
    query_json(&sql)
}

/// Newest `limit` rows of a table. The name is validated against the DB's
/// actual table list — never interpolated raw. WITHOUT ROWID tables get the
/// unordered fallback.
pub fn table_rows(name: &str, limit: usize) -> Result<Vec<Value>> {
    if !table_names()?.iter().any(|n| n == name) {
        bail!("unknown table '{}'", name);
    }
    let q = escape_ident(name);
    let rows = query_json(&format!(
        "SELECT * FROM \"{}\" ORDER BY rowid DESC LIMIT {};",
        q, limit
    ))
    .or_else(|_| query_json(&format!("SELECT * FROM \"{}\" LIMIT {};", q, limit)))?;
    Ok(rows.as_array().cloned().unwrap_or_default())
}

/// Does this column hold SASTR/SASTRUSER string-IDs, and in which scope?
///
/// Heuristic, by design: `*_ID` columns plus the known bare string-ID names
/// (PACKAGE, PUB, DISPLAYVER), minus IDs that are known NOT to be string
/// references (pids, record ids, the string tables' own key). A false
/// positive only adds an annotation when the ID happens to exist in a string
/// table — the raw value is always kept alongside — so erring open is cheap.
pub fn string_id_column(column: &str) -> Option<StrScope> {
    let up = column.to_ascii_uppercase();
    if up == "ACCOUNT_ID" {
        return Some(StrScope::User);
    }
    const NOT_STRING_IDS: &[&str] = &[
        "STRINGID",
        "RECID",
        "PROCESS_ID",
        "PARENT_PROCESS_ID",
        "THREAD_ID",
        "SESSION_ID",
    ];
    if NOT_STRING_IDS.contains(&up.as_str()) {
        return None;
    }
    const BARE_NAMES: &[&str] = &["PACKAGE", "PUB", "DISPLAYVER"];
    if up.ends_with("_ID") || BARE_NAMES.contains(&up.as_str()) {
        return Some(StrScope::System);
    }
    None
}

/// Collect the integer values of every candidate string-ID column in `rows`,
/// split by scope. Non-integer values (already-resolved text, NULLs) are
/// skipped.
fn candidate_ids(rows: &[Value]) -> (BTreeSet<i64>, BTreeSet<i64>) {
    let mut system = BTreeSet::new();
    let mut user = BTreeSet::new();
    for row in rows {
        let Some(obj) = row.as_object() else { continue };
        for (col, val) in obj {
            let Some(scope) = string_id_column(col) else { continue };
            let Some(id) = val.as_i64() else { continue };
            match scope {
                StrScope::System => system.insert(id),
                StrScope::User => user.insert(id),
            };
        }
    }
    (system, user)
}

/// Build the per-column resolution map from the two string-table lookups.
/// System-scope columns prefer SASTR and fall back to SASTRUSER (per-user
/// scan strings live there); ACCOUNT_ID prefers SASTRUSER. Columns where
/// nothing resolved are omitted.
fn pick_resolutions(
    rows: &[Value],
    sastr: &BTreeMap<i64, String>,
    sastruser: &BTreeMap<i64, String>,
) -> ResolvedStrings {
    let mut out = ResolvedStrings::new();
    for row in rows {
        let Some(obj) = row.as_object() else { continue };
        for (col, val) in obj {
            let Some(scope) = string_id_column(col) else { continue };
            let Some(id) = val.as_i64() else { continue };
            let (primary, fallback) = match scope {
                StrScope::System => (sastr, sastruser),
                StrScope::User => (sastruser, sastr),
            };
            if let Some(s) = primary.get(&id).or_else(|| fallback.get(&id)) {
                out.entry(col.clone()).or_default().insert(id, s.clone());
            }
        }
    }
    out
}

/// Resolve every string-ID in `rows` against SASTR/SASTRUSER. Returns a
/// per-column `{id → value}` map covering only IDs that actually resolved.
/// Lookup failures (no string tables — e.g. not an agent DB) resolve to an
/// empty map rather than an error: resolution is an annotation, and a table
/// view without it is still useful.
pub fn resolve_string_ids(rows: &[Value]) -> ResolvedStrings {
    let (system, user) = candidate_ids(rows);
    let all: BTreeSet<i64> = system.union(&user).copied().collect();
    if all.is_empty() {
        return ResolvedStrings::new();
    }
    let sastr = lookup_ids("SASTR", &all).unwrap_or_default();
    let sastruser = lookup_ids("SASTRUSER", &all).unwrap_or_default();
    pick_resolutions(rows, &sastr, &sastruser)
}

/// One `WHERE STRINGID IN (...)` fetch against a string table.
fn lookup_ids(table: &str, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, String>> {
    let list = ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let rows = query_json(&format!(
        "SELECT STRINGID, STRVALUE FROM {} WHERE STRINGID IN ({});",
        table, list
    ))?;
    let mut map = BTreeMap::new();
    if let Some(arr) = rows.as_array() {
        for r in arr {
            if let (Some(id), Some(s)) = (
                r.get("STRINGID").and_then(Value::as_i64),
                r.get("STRVALUE").and_then(Value::as_str),
            ) {
                map.insert(id, s.to_string());
            }
        }
    }
    Ok(map)
}

/// Case-insensitive substring search over both string tables — the fastest
/// "has this endpoint ever seen X in any form" check there is: if nothing in
/// SASTR/SASTRUSER matches, no table can reference it (conclusive absence).
/// `%`/`_` in the pattern act as SQL LIKE wildcards. Returns
/// `{scope, STRINGID, STRVALUE}` rows.
pub fn search_strings(pattern: &str, limit: usize) -> Result<Vec<Value>> {
    let p = escape_str(pattern);
    let both = format!(
        "SELECT 'SASTR' AS scope, STRINGID, STRVALUE FROM SASTR WHERE STRVALUE LIKE '%{p}%' \
         UNION ALL \
         SELECT 'SASTRUSER' AS scope, STRINGID, STRVALUE FROM SASTRUSER WHERE STRVALUE LIKE '%{p}%' \
         LIMIT {limit};"
    );
    // SASTRUSER may not exist on every platform/version — retry system-only.
    let rows = query_json(&both).or_else(|_| {
        query_json(&format!(
            "SELECT 'SASTR' AS scope, STRINGID, STRVALUE FROM SASTR WHERE STRVALUE LIKE '%{p}%' LIMIT {limit};"
        ))
    })?;
    Ok(rows.as_array().cloned().unwrap_or_default())
}

/// SQL string-literal escaping: double every single quote.
fn escape_str(s: &str) -> String {
    s.replace('\'', "''")
}

/// SQL identifier escaping for `"..."` quoting: double every double quote.
fn escape_ident(s: &str) -> String {
    s.replace('"', "\"\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn string_id_column_heuristics() {
        assert_eq!(string_id_column("APP_ID"), Some(StrScope::System));
        assert_eq!(string_id_column("PATH_ID"), Some(StrScope::System));
        assert_eq!(string_id_column("cmdline_id"), Some(StrScope::System));
        assert_eq!(string_id_column("PACKAGE"), Some(StrScope::System));
        assert_eq!(string_id_column("PUB"), Some(StrScope::System));
        assert_eq!(string_id_column("DISPLAYVER"), Some(StrScope::System));
        assert_eq!(string_id_column("ACCOUNT_ID"), Some(StrScope::User));

        // integer IDs that are not string references
        assert_eq!(string_id_column("STRINGID"), None);
        assert_eq!(string_id_column("RECID"), None);
        assert_eq!(string_id_column("PROCESS_ID"), None);
        assert_eq!(string_id_column("SESSION_ID"), None);
        // and plain non-ID columns
        assert_eq!(string_id_column("WFLAGS"), None);
        assert_eq!(string_id_column("START_TIME"), None);
        assert_eq!(string_id_column("PACKAGEGUID"), None);
    }

    #[test]
    fn candidate_ids_split_by_scope_and_skip_non_integers() {
        let rows = vec![
            json!({"APP_ID": 10, "ACCOUNT_ID": 20, "PROCESS_ID": 999, "PACKAGE": 30}),
            json!({"APP_ID": 11, "ACCOUNT_ID": null, "PACKAGE": "already text"}),
        ];
        let (system, user) = candidate_ids(&rows);
        assert_eq!(system, BTreeSet::from([10, 11, 30]));
        assert_eq!(user, BTreeSet::from([20]));
    }

    #[test]
    fn pick_resolutions_prefers_scope_table_and_falls_back() {
        let rows = vec![json!({"APP_ID": 1, "ACCOUNT_ID": 2, "PATH_ID": 3, "RECID": 1})];
        let sastr = BTreeMap::from([(1, "Postman".to_string()), (2, "sys-two".to_string())]);
        let sastruser = BTreeMap::from([(2, "alice".to_string())]);
        let resolved = pick_resolutions(&rows, &sastr, &sastruser);

        assert_eq!(resolved["APP_ID"][&1], "Postman");
        // ACCOUNT_ID prefers SASTRUSER even though SASTR also has id 2
        assert_eq!(resolved["ACCOUNT_ID"][&2], "alice");
        // id 3 resolves nowhere → PATH_ID column omitted entirely
        assert!(!resolved.contains_key("PATH_ID"));
        // RECID is never a string-ID, even when the id would resolve
        assert!(!resolved.contains_key("RECID"));
    }

    #[test]
    fn sql_escaping() {
        assert_eq!(escape_str("O'Brien's"), "O''Brien''s");
        assert_eq!(escape_ident("we\"ird"), "we\"\"ird");
    }
}
