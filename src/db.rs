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
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

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
/// Resolve canonical table name from `sqlite_master`, matching exactly,
/// case-insensitively, or ignoring underscores (so `SAPROCESS_TREE` resolves to `SAPROCESSTREE`).
pub fn resolve_table_name(name: &str) -> Result<String> {
    let names = table_names()?;
    if let Some(exact) = names.iter().find(|n| n.as_str() == name) {
        return Ok(exact.clone());
    }
    if let Some(nocase) = names.iter().find(|n| n.eq_ignore_ascii_case(name)) {
        return Ok(nocase.clone());
    }
    let target_clean = name.to_ascii_uppercase().replace('_', "");
    if let Some(clean) = names.iter().find(|n| n.to_ascii_uppercase().replace('_', "") == target_clean) {
        return Ok(clean.clone());
    }
    bail!("unknown table '{}'", name);
}

/// Newest `limit` rows of a table. The name is validated against the DB's
/// actual table list — never interpolated raw. WITHOUT ROWID tables get the
/// unordered fallback.
pub fn table_rows(name: &str, limit: usize) -> Result<Vec<Value>> {
    let canonical = resolve_table_name(name)?;
    let name = canonical.as_str();
    let q = escape_ident(name);
    let rows_val = query_json(&format!(
        "SELECT * FROM \"{}\" ORDER BY rowid DESC LIMIT {};",
        q, limit
    ))
    .or_else(|_| query_json(&format!("SELECT * FROM \"{}\" LIMIT {};", q, limit)))?;
    let mut rows = rows_val.as_array().cloned().unwrap_or_default();
    enrich_process_tree_rows(name, &mut rows);
    Ok(rows)
}

/// All column names for a given table, ordered by field number.
pub fn table_column_names(name: &str) -> Result<Vec<String>> {
    let canonical = resolve_table_name(name)?;
    let name = canonical.as_str();
    let q = escape_ident(name);
    let rows = query_json(&format!("SELECT name FROM pragma_table_info('{}');", q))?;
    let mut cols: Vec<String> = rows
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r.get("name")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let clean_name = name.to_ascii_uppercase().replace('_', "");
    if clean_name.starts_with("SAPROCESSTREE") {
        if let Some(pos) = cols.iter().position(|c| c.eq_ignore_ascii_case("PROCESS_ID")) {
            cols.insert(pos + 1, "APP_NAME".to_string());
            cols.insert(pos + 2, "PARENTPID".to_string());
            cols.insert(pos + 3, "PARENT_APP_NAME".to_string());
        }
        if let Some(pos) = cols.iter().position(|c| c.eq_ignore_ascii_case("CHILD_PIDS")) {
            cols.insert(pos + 1, "CHILD_APPS".to_string());
        }
    }
    Ok(cols)
}

/// Where to find the C++ table definition headers (`DbSa*.h` / `Struc*.h`).
pub fn header_dir() -> PathBuf {
    if let Ok(p) = std::env::var("LSMAN_HEADERS") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join("src/systrack/Agent/LsiAgent/Tables");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("/home/polo/src/systrack/Agent/LsiAgent/Tables")
}

fn get_table_header_paths() -> &'static BTreeMap<String, (PathBuf, PathBuf)> {
    static MAP: OnceLock<BTreeMap<String, (PathBuf, PathBuf)>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = BTreeMap::new();
        let dir = header_dir();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if fname.starts_with("DbSa") && fname.ends_with(".h") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut tnames = Vec::new();
                        for line in content.lines() {
                            if line.contains("GetTableName") {
                                if let Some(idx) = line.find("_T(\"") {
                                    if let Some(end) = line[idx + 4..].find('"') {
                                        tnames.push(line[idx + 4..idx + 4 + end].to_uppercase());
                                    }
                                }
                            }
                        }
                        if tnames.is_empty() {
                            let mut in_get_table_name = false;
                            for line in content.lines() {
                                if line.contains("GetTableName") {
                                    in_get_table_name = true;
                                } else if in_get_table_name && line.contains('{') {
                                    continue;
                                } else if in_get_table_name && line.contains("_T(\"") {
                                    if let Some(idx) = line.find("_T(\"") {
                                        if let Some(end) = line[idx + 4..].find('"') {
                                            tnames.push(line[idx + 4..idx + 4 + end].to_uppercase());
                                            break;
                                        }
                                    }
                                } else if in_get_table_name && line.contains('}') {
                                    break;
                                }
                            }
                        }
                        if tnames.is_empty() {
                            let stem = fname.strip_prefix("DbSa").unwrap().strip_suffix(".h").unwrap();
                            let derived = if stem.to_ascii_uppercase().starts_with("SA") {
                                stem.to_ascii_uppercase()
                            } else {
                                format!("SA{}", stem.to_ascii_uppercase())
                            };
                            tnames.push(derived);
                        }
                        let mut struc_path = PathBuf::new();
                        for line in content.lines() {
                            if line.trim().starts_with("#include") && line.contains("Struc") {
                                if let Some(idx) = line.find('"') {
                                    if let Some(end) = line[idx + 1..].find('"') {
                                        let sf = &line[idx + 1..idx + 1 + end];
                                        let sp = dir.join(sf);
                                        if sp.exists() {
                                            struc_path = sp;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        for tname in tnames {
                            map.insert(tname.clone(), (path.clone(), struc_path.clone()));
                            let clean = tname.replace('_', "");
                            if clean != tname {
                                map.insert(clean, (path.clone(), struc_path.clone()));
                            }
                        }
                    }
                }
            }
        }
        map
    })
}

/// Parse schema field descriptions from C++ table header files (`~/src/systrack/Agent/LsiAgent/Tables`).
/// Maps DB column names (`PROCESS_ID`) -> description (`"id of process for app"`).
pub fn table_descriptions(name: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let clean_name = name.to_ascii_uppercase().replace('_', "");
    let Some((dbsa_path, struc_path)) = get_table_header_paths()
        .get(&name.to_ascii_uppercase())
        .or_else(|| get_table_header_paths().get(&clean_name))
    else {
        return out;
    };
    let (Ok(dbsa_content), Ok(struc_content)) = (std::fs::read_to_string(dbsa_path), std::fs::read_to_string(struc_path)) else {
        return out;
    };

    // 1. Map column names to C++ struct field names via ADO binding entries
    let mut col_to_field = BTreeMap::new();
    for line in dbsa_content.lines() {
        if line.contains("ADO_") && line.contains("m_struc.") {
            if let Some(struc_idx) = line.find("m_struc.") {
                let rest = &line[struc_idx + 8..];
                let field_name = rest.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').next().unwrap_or("");
                if !field_name.is_empty() {
                    if let Some(ord_idx) = line.find("_ORD") {
                        let before = &line[..ord_idx];
                        if let Some(col_name) = before.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').rfind(|s| !s.is_empty()) {
                            col_to_field.insert(col_name.to_ascii_uppercase(), field_name.to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. Scan Struc*.h for struct fields and preceding comments
    let mut field_to_comment = BTreeMap::new();
    let mut last_comments = Vec::new();
    for line in struc_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            let c = trimmed.strip_prefix("//").unwrap().trim();
            if !c.is_empty() && !c.starts_with('*') && !c.starts_with("COL-") {
                last_comments.push(c.to_string());
            }
        } else if trimmed.is_empty() {
            // keep collecting comments across blank lines unless there's a struct break
        } else if trimmed.starts_with("struct ") || trimmed.starts_with("enum ") || trimmed.starts_with("class ") || trimmed.starts_with('{') {
            last_comments.clear();
        } else {
            if let Some(semi_idx) = trimmed.find(';') {
                let before_semi = &trimmed[..semi_idx];
                let before_arr = match before_semi.find('[') {
                    Some(i) => &before_semi[..i],
                    None => before_semi,
                };
                if let Some(field_name) = before_arr.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').rfind(|s| !s.is_empty()) {
                    if !last_comments.is_empty() && !field_name.ends_with("Stat") {
                        field_to_comment.insert(field_name.to_string(), last_comments.join(" "));
                    }
                }
            }
            last_comments.clear();
        }
    }

    // 3. Combine column name -> struct field -> comment
    for (col, field) in col_to_field {
        if let Some(comment) = field_to_comment.get(&field) {
            out.insert(col, comment.clone());
        }
    }
    if clean_name.starts_with("SAPROCESSTREE") {
        out.insert("APP_NAME".to_string(), "Application exe file name (automatically correlated from SAAPP via PROCESS_ID)".to_string());
        out.insert("PARENTPID".to_string(), "PID of parent process for app (correlated from SAAPP via PROCESS_ID)".to_string());
        out.insert("PARENT_APP_NAME".to_string(), "Parent application exe file name (correlated from parent SAAPP record via PARENTPID)".to_string());
        out.insert("CHILD_APPS".to_string(), "Child application exe file names across the process tree (correlated from SAAPP via CHILD_PIDS)".to_string());
    }
    out
}

/// Rich table query result including rows, total/filtered counts, pagination metadata, string resolutions, and column descriptions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TableQueryResult {
    pub name: String,
    pub total_rows: usize,
    pub filtered_rows: usize,
    pub limit: usize,
    pub offset: usize,
    pub sort: String,
    pub dir: String,
    pub rows: Vec<Value>,
    pub resolved: ResolvedStrings,
    pub descriptions: BTreeMap<String, String>,
}

/// Query table rows with pagination, sorting, and full-text / string-ID filtering.
pub fn query_table(
    name: &str,
    limit: usize,
    offset: usize,
    sort: &str,
    dir: &str,
    filter: &str,
) -> Result<TableQueryResult> {
    let canonical = resolve_table_name(name)?;
    let name = canonical.as_str();
    let q_name = escape_ident(name);
    let total_rows = match query_json(&format!("SELECT count(*) AS n FROM \"{}\";", q_name)) {
        Ok(v) => v
            .as_array()
            .and_then(|a| a.first())
            .and_then(|o| o.get("n"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as usize,
        Err(_) => 0,
    };

    let col_names = table_column_names(name)?;
    let sort_clean = if sort.is_empty() || sort.eq_ignore_ascii_case("rowid") {
        "rowid".to_string()
    } else if col_names.iter().any(|c| c.eq_ignore_ascii_case(sort)) {
        escape_ident(sort)
    } else {
        "rowid".to_string()
    };
    let dir_clean = if dir.eq_ignore_ascii_case("asc") { "ASC" } else { "DESC" };

    let filter_str = filter.trim();
    let where_clause = if !filter_str.is_empty() && !col_names.is_empty() {
        let matching_string_ids = search_strings(filter_str, 200).unwrap_or_default();
        let ids: Vec<String> = matching_string_ids
            .iter()
            .filter_map(|r| r.get("STRINGID").and_then(Value::as_i64).map(|i| i.to_string()))
            .collect();

        let mut conditions = Vec::new();
        let escaped_filter = escape_str(filter_str);
        for col in &col_names {
            conditions.push(format!("CAST(\"{}\" AS TEXT) LIKE '%{}%'", escape_ident(col), escaped_filter));
            if string_id_column(col).is_some() && !ids.is_empty() {
                conditions.push(format!("\"{}\" IN ({})", escape_ident(col), ids.join(",")));
            }
        }
        format!(" WHERE {}", conditions.join(" OR "))
    } else {
        String::new()
    };

    let filtered_rows = if where_clause.is_empty() {
        total_rows
    } else {
        match query_json(&format!("SELECT count(*) AS n FROM \"{}\"{};", q_name, where_clause)) {
            Ok(v) => v
                .as_array()
                .and_then(|a| a.first())
                .and_then(|o| o.get("n"))
                .and_then(Value::as_i64)
                .unwrap_or(0) as usize,
            Err(_) => 0,
        }
    };

    let order_by = if sort_clean.eq_ignore_ascii_case("rowid") {
        format!("rowid {}", dir_clean)
    } else {
        format!("\"{}\" {}", sort_clean, dir_clean)
    };

    let sql = format!(
        "SELECT * FROM \"{}\"{} ORDER BY {} LIMIT {} OFFSET {};",
        q_name, where_clause, order_by, limit, offset
    );
    let mut rows = match query_json(&sql) {
        Ok(v) => v.as_array().cloned().unwrap_or_default(),
        Err(_) => {
            let fallback_sql = format!(
                "SELECT * FROM \"{}\"{} LIMIT {} OFFSET {};",
                q_name, where_clause, limit, offset
            );
            query_json(&fallback_sql)?.as_array().cloned().unwrap_or_default()
        }
    };

    enrich_process_tree_rows(name, &mut rows);
    let resolved = resolve_string_ids(&rows);
    let descriptions = table_descriptions(name);

    Ok(TableQueryResult {
        name: name.to_string(),
        total_rows,
        filtered_rows,
        limit,
        offset,
        sort: if sort_clean == "rowid" { "rowid".to_string() } else { sort.to_string() },
        dir: dir_clean.to_lowercase(),
        rows,
        resolved,
        descriptions,
    })
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
    let mut out = if all.is_empty() {
        ResolvedStrings::new()
    } else {
        let sastr = lookup_ids("SASTR", &all).unwrap_or_default();
        let sastruser = lookup_ids("SASTRUSER", &all).unwrap_or_default();
        pick_resolutions(rows, &sastr, &sastruser)
    };
    resolve_process_and_tree_ids(rows, &mut out);
    out
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

/// Enrich rows for `SAPROCESSTREE` tables by correlating application names and parent PIDs from `SAAPP`.
pub fn enrich_process_tree_rows(table_name: &str, rows: &mut [Value]) {
    let clean_name = table_name.to_ascii_uppercase().replace('_', "");
    if !clean_name.starts_with("SAPROCESSTREE") || rows.is_empty() {
        return;
    }

    let mut pids = BTreeSet::new();
    for row in rows.iter() {
        let Some(obj) = row.as_object() else { continue };
        if let Some(pid) = obj.get("PROCESS_ID").or_else(|| obj.get("process_Id")).and_then(|v| v.as_i64()) {
            if pid > 0 { pids.insert(pid); }
        }
        if let Some(cp_str) = obj.get("CHILD_PIDS").or_else(|| obj.get("child_Pids")).and_then(|v| v.as_str()) {
            for part in cp_str.split(',') {
                if let Ok(id) = part.trim().parse::<i64>() {
                    if id > 0 { pids.insert(id); }
                }
            }
        }
    }

    if pids.is_empty() {
        return;
    }

    let mut pid_to_app = BTreeMap::new();
    let mut pid_to_parent = BTreeMap::new();
    let mut pid_to_parent_app = BTreeMap::new();
    let pids_vec: Vec<i64> = pids.iter().copied().collect();
    for chunk in pids_vec.chunks(500) {
        let list = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT a.PROCESS_ID, a.PARENTPID, s.STRVALUE as APP_NAME, sp.STRVALUE as PARENT_APP_NAME \
             FROM SAAPP a \
             LEFT JOIN SASTR s ON a.APP_ID = s.STRINGID \
             LEFT JOIN SAAPP ap ON a.PARENTPID = ap.PROCESS_ID \
             LEFT JOIN SASTR sp ON ap.APP_ID = sp.STRINGID \
             WHERE a.PROCESS_ID IN ({0}) OR a.PARENTPID IN ({0}) \
             GROUP BY a.PROCESS_ID;",
            list
        );
        if let Ok(val) = query_json(&sql) {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    let pid = item.get("PROCESS_ID").and_then(|v| v.as_i64());
                    let parent_pid = item.get("PARENTPID").and_then(|v| v.as_i64());
                    let app_name = item.get("APP_NAME").and_then(|v| v.as_str());
                    let parent_app = item.get("PARENT_APP_NAME").and_then(|v| v.as_str());
                    if let Some(p) = pid {
                        if let Some(name) = app_name {
                            pid_to_app.insert(p, name.to_string());
                        }
                        if let Some(pp) = parent_pid {
                            pid_to_parent.insert(p, pp);
                        }
                        if let Some(pname) = parent_app {
                            pid_to_parent_app.insert(p, pname.to_string());
                        }
                    }
                }
            }
        }
    }

    for row in rows.iter_mut() {
        let Some(obj) = row.as_object_mut() else { continue };
        let pid = obj.get("PROCESS_ID").or_else(|| obj.get("process_Id")).and_then(|v| v.as_i64()).unwrap_or(0);
        
        let app_name = pid_to_app.get(&pid).cloned().unwrap_or_else(|| format!("PID {}", pid));
        let parent_pid = pid_to_parent.get(&pid).copied();
        let parent_app = match parent_pid {
            Some(pp) => pid_to_parent_app.get(&pid).cloned().or_else(|| pid_to_app.get(&pp).cloned()).unwrap_or_else(|| format!("PID {}", pp)),
            None => "Unknown".to_string(),
        };

        let mut new_obj = serde_json::Map::with_capacity(obj.len() + 4);
        for (k, v) in obj.iter() {
            new_obj.insert(k.clone(), v.clone());
            if k.eq_ignore_ascii_case("PROCESS_ID") {
                new_obj.insert("APP_NAME".to_string(), Value::String(app_name.clone()));
                new_obj.insert("PARENTPID".to_string(), match parent_pid { Some(pp) => Value::Number(pp.into()), None => Value::Null });
                new_obj.insert("PARENT_APP_NAME".to_string(), Value::String(parent_app.clone()));
            } else if k.eq_ignore_ascii_case("CHILD_PIDS") {
                let cp_resolved = if let Some(cp_str) = v.as_str() {
                    let parts: Vec<String> = cp_str.split(',')
                        .filter_map(|s| {
                            let t = s.trim();
                            if t.is_empty() { return None; }
                            if let Ok(id) = t.parse::<i64>() {
                                if let Some(name) = pid_to_app.get(&id) {
                                    return Some(format!("{} [{}]", name, id));
                                }
                            }
                            Some(t.to_string())
                        })
                        .collect();
                    parts.join(", ")
                } else {
                    String::new()
                };
                new_obj.insert("CHILD_APPS".to_string(), Value::String(cp_resolved));
            }
        }
        *obj = new_obj;
    }
}

/// Resolve process IDs (`PROCESS_ID`, `PARENTPID`, etc.) inside any table against `SAAPP`.
fn resolve_process_and_tree_ids(rows: &[Value], out: &mut ResolvedStrings) {
    let mut pids = BTreeSet::new();
    for row in rows {
        let Some(obj) = row.as_object() else { continue };
        for (col, val) in obj {
            let up = col.to_ascii_uppercase();
            if up == "PROCESS_ID" || up == "PARENTPID" || up == "PARENT_PROCESS_ID" || up == "IDXPID" || up == "IDXPID2" || up == "CHILD_PIDS" {
                if let Some(id) = val.as_i64() {
                    if id > 0 { pids.insert(id); }
                } else if let Some(s) = val.as_str() {
                    for part in s.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
                        if let Ok(id) = part.trim().parse::<i64>() {
                            if id > 0 { pids.insert(id); }
                        }
                    }
                }
            }
        }
    }
    if pids.is_empty() {
        return;
    }

    let mut pid_to_app = BTreeMap::new();
    let pids_vec: Vec<i64> = pids.iter().copied().collect();
    for chunk in pids_vec.chunks(500) {
        let list = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT a.PROCESS_ID, s.STRVALUE as APP_NAME \
             FROM SAAPP a \
             LEFT JOIN SASTR s ON a.APP_ID = s.STRINGID \
             WHERE a.PROCESS_ID IN ({}) \
             GROUP BY a.PROCESS_ID;",
            list
        );
        if let Ok(val) = query_json(&sql) {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    if let (Some(pid), Some(name)) = (
                        item.get("PROCESS_ID").and_then(|v| v.as_i64()),
                        item.get("APP_NAME").and_then(|v| v.as_str()),
                    ) {
                        pid_to_app.insert(pid, name.to_string());
                    }
                }
            }
        }
    }

    for row in rows {
        let Some(obj) = row.as_object() else { continue };
        for (col, val) in obj {
            let up = col.to_ascii_uppercase();
            if up == "PROCESS_ID" || up == "IDXPID" || up == "IDXPID2" {
                if let Some(id) = val.as_i64() {
                    if let Some(name) = pid_to_app.get(&id) {
                        out.entry(col.clone()).or_default().insert(id, name.clone());
                    }
                }
            } else if up == "PARENTPID" || up == "PARENT_PROCESS_ID" {
                if let Some(id) = val.as_i64() {
                    if let Some(name) = pid_to_app.get(&id) {
                        out.entry(col.clone()).or_default().insert(id, format!("{} [PID {}]", name, id));
                    }
                }
            }
        }
    }
}

/// Query and reconstruct full process ancestry and tree children for a given PID or application name pattern.
pub fn query_process_tree(pattern_or_pid: &str) -> Result<Value> {
    let clean = pattern_or_pid.trim();
    if clean.is_empty() {
        bail!("pattern or pid cannot be empty");
    }

    let sql_target = if let Ok(pid) = clean.parse::<i64>() {
        format!(
            "SELECT a.PROCESS_ID, a.PARENTPID, a.START_TIME, a.END_TIME, a.EXIT_CODE, \
             s.STRVALUE as APP_NAME, p.STRVALUE as PATH_NAME, c.STRVALUE as CMDLINE \
             FROM SAAPP a \
             LEFT JOIN SASTR s ON a.APP_ID = s.STRINGID \
             LEFT JOIN SASTR p ON a.PATH_ID = p.STRINGID \
             LEFT JOIN SASTR c ON a.CMDLINE_ID = c.STRINGID \
             WHERE a.PROCESS_ID = {} OR a.PARENTPID = {} \
             ORDER BY a.START_TIME DESC LIMIT 15;",
            pid, pid
        )
    } else {
        let q = escape_str(clean);
        format!(
            "SELECT a.PROCESS_ID, a.PARENTPID, a.START_TIME, a.END_TIME, a.EXIT_CODE, \
             s.STRVALUE as APP_NAME, p.STRVALUE as PATH_NAME, c.STRVALUE as CMDLINE \
             FROM SAAPP a \
             LEFT JOIN SASTR s ON a.APP_ID = s.STRINGID \
             LEFT JOIN SASTR p ON a.PATH_ID = p.STRINGID \
             LEFT JOIN SASTR c ON a.CMDLINE_ID = c.STRINGID \
             WHERE s.STRVALUE LIKE '%{}%' OR p.STRVALUE LIKE '%{}%' OR c.STRVALUE LIKE '%{}%' \
             ORDER BY a.START_TIME DESC LIMIT 15;",
            q, q, q
        )
    };

    let target_rows = query_json(&sql_target)?.as_array().cloned().unwrap_or_default();
    if target_rows.is_empty() {
        bail!("No matching process found for '{}'", clean);
    }

    let mut results = Vec::new();
    for target in target_rows {
        let Some(pid) = target.get("PROCESS_ID").and_then(|v| v.as_i64()) else { continue };
        let mut ancestry = Vec::new();
        let mut cur_parent = target.get("PARENTPID").and_then(|v| v.as_i64()).unwrap_or(0);
        let mut cur_start = target.get("START_TIME").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let mut depth = 0;
        while cur_parent > 0 && depth < 20 {
            depth += 1;
            let time_filter = if !cur_start.is_empty() {
                format!(" AND START_TIME <= '{}'", escape_str(&cur_start))
            } else {
                String::new()
            };
            let sql_anc = format!(
                "SELECT a.PROCESS_ID, a.PARENTPID, a.START_TIME, a.END_TIME, \
                 s.STRVALUE as APP_NAME, p.STRVALUE as PATH_NAME, c.STRVALUE as CMDLINE \
                 FROM SAAPP a \
                 LEFT JOIN SASTR s ON a.APP_ID = s.STRINGID \
                 LEFT JOIN SASTR p ON a.PATH_ID = p.STRINGID \
                 LEFT JOIN SASTR c ON a.CMDLINE_ID = c.STRINGID \
                 WHERE a.PROCESS_ID = {}{} \
                 ORDER BY a.START_TIME DESC LIMIT 1;",
                cur_parent, time_filter
            );
            if let Ok(anc_val) = query_json(&sql_anc) {
                if let Some(anc_arr) = anc_val.as_array() {
                    if let Some(anc_row) = anc_arr.first() {
                        ancestry.push(anc_row.clone());
                        cur_parent = anc_row.get("PARENTPID").and_then(|v| v.as_i64()).unwrap_or(0);
                        cur_start = anc_row.get("START_TIME").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        continue;
                    }
                }
            }
            break;
        }
        ancestry.reverse(); // Top-most ancestor first

        let sql_tree = format!(
            "SELECT CHILD_PIDS, TOTAL_TIME, ACTIV_TIME, KERN_USED, USER_USED, MEM_USED, IOREADS, IOWRITES \
             FROM SAPROCESSTREE WHERE PROCESS_ID = {} ORDER BY START_TIME DESC LIMIT 1;",
            pid
        );
        let tree_info = query_json(&sql_tree)?.as_array().and_then(|a| a.first().cloned());

        let mut children = Vec::new();
        if let Some(ref ti) = tree_info {
            if let Some(cp_str) = ti.get("CHILD_PIDS").and_then(|v| v.as_str()) {
                let pids: Vec<i64> = cp_str
                    .split(',')
                    .filter_map(|s| s.trim().parse::<i64>().ok())
                    .collect();
                if !pids.is_empty() {
                    let list = pids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
                    let sql_children = format!(
                        "SELECT a.PROCESS_ID, s.STRVALUE as APP_NAME, p.STRVALUE as PATH_NAME \
                         FROM SAAPP a \
                         LEFT JOIN SASTR s ON a.APP_ID = s.STRINGID \
                         LEFT JOIN SASTR p ON a.PATH_ID = p.STRINGID \
                         WHERE a.PROCESS_ID IN ({}) \
                         GROUP BY a.PROCESS_ID;",
                        list
                    );
                    if let Ok(cval) = query_json(&sql_children) {
                        if let Some(carr) = cval.as_array() {
                            children = carr.clone();
                        }
                    }
                }
            }
        }

        results.push(serde_json::json!({
            "target": target,
            "ancestry": ancestry,
            "tree_metrics": tree_info,
            "children": children,
        }));
    }

    Ok(serde_json::json!({
        "query": pattern_or_pid,
        "count": results.len(),
        "trees": results,
    }))
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

    #[test]
    fn table_descriptions_from_headers() {
        let descs = table_descriptions("SAAPP");
        if header_dir().exists() {
            assert!(descs.contains_key("PROCESS_ID"), "expected PROCESS_ID description, got {:?}", descs);
            assert_eq!(descs.get("PROCESS_ID").map(String::as_str), Some("id of process for app"));
            assert!(descs.contains_key("START_TIME"));
        }
    }

    #[test]
    fn process_tree_virtual_columns_inject() {
        let cols = table_column_names("SAPROCESSTREE").unwrap_or_default();
        if !cols.is_empty() {
            assert!(cols.contains(&"APP_NAME".to_string()));
            assert!(cols.contains(&"PARENTPID".to_string()));
            assert!(cols.contains(&"PARENT_APP_NAME".to_string()));
            assert!(cols.contains(&"CHILD_APPS".to_string()));
        }

        let descs = table_descriptions("SAPROCESSTREE");
        assert!(descs.contains_key("APP_NAME"));
        assert!(descs.contains_key("PARENT_APP_NAME"));
        assert!(descs.contains_key("CHILD_APPS"));

        // Verify that SAPROCESS_TREE (with underscore) normalizes cleanly
        let descs_underscore = table_descriptions("SAPROCESS_TREE");
        assert!(descs_underscore.contains_key("APP_NAME"));
        assert!(descs_underscore.contains_key("PARENT_APP_NAME"));
        assert!(descs_underscore.contains_key("CHILD_APPS"));
    }
}
