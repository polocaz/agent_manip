//! `lsman serve` — a local web dashboard over the same data the CLI/TUI
//! expose: daemon status, uplink, error triage, log tails — plus daemon
//! start/stop/restart and downloads of the agent's config/db/profile/logs.
//!
//! The dashboard is unauthenticated, and the control/download/config-edit
//! endpoints make that matter: it binds 127.0.0.1 unless told otherwise; use
//! an SSH port forward for remote boxes. Mutating endpoints require POST plus
//! an `X-Lsman-Control` header (forces a CORS preflight, so a malicious page
//! in the user's browser can't fire them cross-origin). Config writes are
//! additionally refused while the daemon is running — it only reads
//! lsiagent.cfg at startup, so editing under a live agent invites confusion.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use serde::Serialize;

use crate::daemon::DaemonManager;
use crate::paths;
use crate::report;
use crate::triage;

const DASHBOARD_HTML: &str = include_str!("../assets/dashboard.html");
/// Per-request read cap on a log tail; agent logs can be hundreds of MB.
const TAIL_READ_CAP: u64 = 1024 * 1024;
const MAX_TAIL_LINES: usize = 2000;

pub struct ServeOptions {
    pub bind: String,
    pub port: u16,
}

pub async fn run(opts: ServeOptions) -> Result<()> {
    let addr = format!("{}:{}", opts.bind, opts.port);
    let server = tiny_http::Server::http(&addr)
        .map_err(|e| anyhow!("failed to bind {}: {}", addr, e))?;
    let server = Arc::new(server);

    println!("lsman web UI: http://{}", addr);
    if opts.bind != "127.0.0.1" && opts.bind != "localhost" {
        println!("warning: bound to {} — the dashboard is unauthenticated and can start/stop the daemon; prefer 127.0.0.1 + SSH port forward", opts.bind);
    }
    if unsafe { libc::geteuid() } != 0 {
        println!("note: not running as root — daemon control/stats, sockets and some logs will be unavailable (use sudo)");
    }
    println!("dashboard includes daemon control, file downloads and config editing (config is editable only while the daemon is stopped); Ctrl-C to stop");

    let mgr = Arc::new(Mutex::new(DaemonManager::new()?));
    let handle = tokio::runtime::Handle::current();

    let mut workers = Vec::new();
    for _ in 0..4 {
        let server = Arc::clone(&server);
        let mgr = Arc::clone(&mgr);
        let handle = handle.clone();
        workers.push(std::thread::spawn(move || {
            while let Ok(request) = server.recv() {
                handle_request(request, &mgr, &handle);
            }
        }));
    }

    // Park the async task; the worker threads own the server from here.
    tokio::task::spawn_blocking(move || {
        for w in workers {
            let _ = w.join();
        }
    })
    .await?;
    Ok(())
}

fn handle_request(
    mut request: tiny_http::Request,
    mgr: &Arc<Mutex<DaemonManager>>,
    handle: &tokio::runtime::Handle,
) {
    let url = request.url().to_string();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url.as_str(), ""),
    };

    let response = match path {
        "/" | "/index.html" => html_response(DASHBOARD_HTML),
        "/api/snapshot" => {
            let mut mgr = mgr.lock().unwrap();
            let snap = handle.block_on(report::gather(&mut mgr));
            json_response(&snap)
        }
        "/api/errors" => {
            let since_label = query_param(query, "since").filter(|s| s != "all");
            let since = since_label.as_deref().and_then(triage::parse_since);
            let scan = report::scan_errors(since, since_label);
            json_response(&scan)
        }
        "/api/log" => match api_log(query) {
            Ok(resp) => resp,
            Err(e) => error_response(400, &e.to_string()),
        },
        "/api/download" => match api_download(query) {
            Ok(resp) => resp,
            Err(e) => error_response(400, &e.to_string()),
        },
        "/api/crash" => match api_crash(query) {
            Ok(resp) => resp,
            Err(e) => error_response(400, &e.to_string()),
        },
        "/api/collect" => match api_collect(query, handle) {
            Ok(resp) => resp,
            Err(e) => error_response(500, &e.to_string()),
        },
        "/api/config" => match *request.method() {
            tiny_http::Method::Get => api_config_get(),
            tiny_http::Method::Post => {
                if !has_control_header(&request) {
                    error_response(403, "missing X-Lsman-Control header")
                } else {
                    api_config_post(&mut request, mgr, handle)
                }
            }
            _ => error_response(405, "GET or POST required"),
        },
        "/api/config/trace" => {
            if *request.method() != tiny_http::Method::Post {
                error_response(405, "POST required")
            } else if !has_control_header(&request) {
                error_response(403, "missing X-Lsman-Control header")
            } else {
                api_trace_post(query, mgr, handle)
            }
        }
        "/api/db/tables" => match api_db_tables() {
            Ok(resp) => resp,
            Err(e) => error_response(400, &e.to_string()),
        },
        "/api/db/table" => match api_db_table(query) {
            Ok(resp) => resp,
            Err(e) => error_response(400, &e.to_string()),
        },
        "/api/db/strings" => match api_db_strings(query) {
            Ok(resp) => resp,
            Err(e) => error_response(400, &e.to_string()),
        },
        "/api/daemon/start" | "/api/daemon/stop" | "/api/daemon/restart" => {
            if *request.method() != tiny_http::Method::Post {
                error_response(405, "POST required")
            } else if !has_control_header(&request) {
                error_response(403, "missing X-Lsman-Control header")
            } else {
                api_daemon(path, mgr, handle)
            }
        }
        _ => error_response(404, "not found"),
    };

    let _ = request.respond(response);
}

/// The custom header the dashboard sends on control POSTs. Cross-origin pages
/// can't add it without a CORS preflight (which we never approve), so its
/// presence proves the request came from the dashboard or a deliberate client.
fn has_control_header(request: &tiny_http::Request) -> bool {
    request
        .headers()
        .iter()
        .any(|h| h.field.equiv("x-lsman-control"))
}

#[derive(Serialize)]
struct ControlResult {
    ok: bool,
    action: String,
    state: String,
    error: Option<String>,
}

fn api_daemon(
    path: &str,
    mgr: &Arc<Mutex<DaemonManager>>,
    handle: &tokio::runtime::Handle,
) -> HttpResponse {
    let action = path.rsplit('/').next().unwrap_or_default().to_string();
    let mut mgr = mgr.lock().unwrap();
    // Refresh first so "already running" style checks act on current state.
    handle.block_on(mgr.update_status());
    let result = match action.as_str() {
        "start" => mgr.start_daemon(),
        "stop" => mgr.stop_daemon(),
        "restart" => mgr.restart_daemon(),
        _ => Err(anyhow!("unknown action '{}'", action)),
    };
    handle.block_on(mgr.update_status());
    let state = mgr.get_state().to_string();
    match result {
        Ok(()) => json_response(&ControlResult {
            ok: true,
            action,
            state,
            error: None,
        }),
        Err(e) => json_response(&ControlResult {
            ok: false,
            action,
            state,
            error: Some(e.to_string()),
        })
        .with_status_code(500),
    }
}

/// Config bodies are tiny (tens of KB); anything near this is not lsiagent.cfg.
const CONFIG_WRITE_CAP: usize = 1024 * 1024;

#[derive(Serialize)]
struct ConfigGet {
    path: String,
    exists: bool,
    content: String,
}

fn api_config_get() -> HttpResponse {
    let path = paths::config_path();
    match fs::read_to_string(&path) {
        Ok(content) => json_response(&ConfigGet {
            path: path.display().to_string(),
            exists: true,
            content,
        }),
        // Missing config is a valid state (fresh install) — editable as empty.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json_response(&ConfigGet {
            path: path.display().to_string(),
            exists: false,
            content: String::new(),
        }),
        Err(e) => error_response(400, &open_error(&path, e).to_string()),
    }
}

#[derive(Serialize)]
struct ConfigSaved {
    ok: bool,
    bytes_written: usize,
    trace_level: Option<i64>,
}

fn api_config_post(
    request: &mut tiny_http::Request,
    mgr: &Arc<Mutex<DaemonManager>>,
    handle: &tokio::runtime::Handle,
) -> HttpResponse {
    let mut body = Vec::new();
    if let Err(e) = request
        .as_reader()
        .take(CONFIG_WRITE_CAP as u64 + 1)
        .read_to_end(&mut body)
    {
        return error_response(400, &format!("reading request body: {}", e));
    }
    if body.len() > CONFIG_WRITE_CAP {
        return error_response(413, "config body too large");
    }
    let content = match String::from_utf8(body) {
        Ok(c) => c,
        Err(_) => return error_response(400, "config must be valid UTF-8 text"),
    };

    // Hold the manager lock across the check + write so lsman itself can't
    // start the daemon mid-save. (The daemon only reads the config at startup,
    // so an edit under a running agent would silently not apply.)
    let mut mgr = mgr.lock().unwrap();
    handle.block_on(mgr.update_status());
    if mgr.get_state() == crate::daemon::DaemonState::Running {
        return error_response(
            409,
            "agent is running — stop it before editing the config (it only reads lsiagent.cfg at startup)",
        );
    }
    if let Err(e) = write_config_atomic(&content) {
        return error_response(500, &e.to_string());
    }
    json_response(&ConfigSaved {
        ok: true,
        bytes_written: content.len(),
        trace_level: crate::cli::read_log_level(&content),
    })
}

/// Quick trace-level set: rewrite `[Debug] LogLevel2` in place via the same
/// tested edit logic the CLI uses, under the same stopped-daemon rule as full
/// config writes.
fn api_trace_post(
    query: &str,
    mgr: &Arc<Mutex<DaemonManager>>,
    handle: &tokio::runtime::Handle,
) -> HttpResponse {
    let level: i64 = match query_param(query, "level").and_then(|v| v.parse().ok()) {
        Some(l) if (0..=8).contains(&l) => l,
        _ => return error_response(400, "need ?level=0..8"),
    };
    let mut mgr = mgr.lock().unwrap();
    handle.block_on(mgr.update_status());
    if mgr.get_state() == crate::daemon::DaemonState::Running {
        return error_response(
            409,
            "agent is running — stop it before editing the config (it only reads lsiagent.cfg at startup)",
        );
    }
    let current = match fs::read_to_string(paths::config_path()) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return error_response(400, &open_error(&paths::config_path(), e).to_string()),
    };
    let updated = crate::cli::set_log_level(&current, level);
    if let Err(e) = write_config_atomic(&updated) {
        return error_response(500, &e.to_string());
    }
    json_response(&ConfigSaved {
        ok: true,
        bytes_written: updated.len(),
        trace_level: Some(level),
    })
}

/// Write via temp file + rename in the same directory so a crash mid-write
/// can't leave a half-written lsiagent.cfg.
fn write_config_atomic(content: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let cfg = paths::config_path();
    let tmp = cfg.with_file_name("lsiagent.cfg.lsman-tmp");
    fs::write(&tmp, content).map_err(|e| write_error(&tmp, e))?;
    // Match the agent installer's world-readable config mode.
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644))?;
    fs::rename(&tmp, &cfg).map_err(|e| {
        fs::remove_file(&tmp).ok();
        write_error(&cfg, e)
    })?;
    Ok(())
}

fn write_error(path: &Path, e: std::io::Error) -> anyhow::Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied && unsafe { libc::geteuid() } != 0 {
        anyhow!(
            "permission denied writing {} — run lsman serve with sudo",
            path.display()
        )
    } else {
        anyhow!("writing {}: {}", path.display(), e)
    }
}

/// Map a `?file=` key to (path on disk, download filename).
/// Keys: `cfg`, `db`, `profile`, `log:<short name>`.
fn download_target(key: &str) -> Option<(PathBuf, String)> {
    if let Some(name) = key.strip_prefix("log:") {
        let log = paths::find_log(name)?;
        let fname = log.path.file_name()?.to_string_lossy().into_owned();
        return Some((log.path, fname));
    }
    match key {
        "cfg" => Some((paths::config_path(), "lsiagent.cfg".into())),
        "db" => Some((paths::database_path(), "collect.sqlite3".into())),
        "profile" => Some((paths::profile_path(), "profile".into())),
        _ => None,
    }
}

fn api_download(query: &str) -> Result<HttpResponse> {
    let key = query_param(query, "file").ok_or_else(|| anyhow!("missing ?file="))?;
    let (path, filename) =
        download_target(&key).ok_or_else(|| anyhow!("unknown file '{}'", key))?;

    // A live agent writes the DB constantly (WAL mode); serve a consistent
    // `sqlite3 .backup` snapshot when possible instead of the raw file.
    if key == "db" {
        match db_snapshot_response(&path, &filename) {
            Ok(resp) => return Ok(resp),
            Err(e) => eprintln!("db snapshot failed ({}); falling back to raw copy", e),
        }
    }
    file_download_response(&path, &filename)
}

/// Stream a file as an attachment without buffering it (logs can be huge).
fn file_download_response(path: &Path, filename: &str) -> Result<HttpResponse> {
    let file = fs::File::open(path).map_err(|e| open_error(path, e))?;
    Ok(attachment(tiny_http::Response::from_file(file), filename))
}

static DB_SNAPSHOT_SEQ: AtomicU64 = AtomicU64::new(0);

fn db_snapshot_response(db: &Path, filename: &str) -> Result<HttpResponse> {
    if !db.exists() {
        bail!("{} does not exist", db.display());
    }
    let tmp = std::env::temp_dir().join(format!(
        "lsman-db-snapshot-{}-{}.sqlite3",
        std::process::id(),
        DB_SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let out = Command::new("sqlite3")
        .arg("-readonly")
        .arg(db)
        .arg(format!(".backup '{}'", tmp.display()))
        .output()
        .map_err(|e| anyhow!("running sqlite3: {}", e))?;
    if !out.status.success() {
        fs::remove_file(&tmp).ok();
        bail!(
            "sqlite3 .backup failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let file = fs::File::open(&tmp)?;
    // Unlink now; the open handle keeps the bytes alive until streamed.
    fs::remove_file(&tmp).ok();
    Ok(attachment(tiny_http::Response::from_file(file), filename))
}

fn attachment(resp: tiny_http::Response<fs::File>, filename: &str) -> HttpResponse {
    resp.with_header(header("Content-Type", "application/octet-stream"))
        .with_header(header(
            "Content-Disposition",
            &format!("attachment; filename=\"{}\"", filename),
        ))
        .boxed()
}

fn api_db_tables() -> Result<HttpResponse> {
    let tables = crate::db::table_counts()?;
    Ok(json_response(&serde_json::json!({ "tables": tables })))
}

fn api_db_table(query: &str) -> Result<HttpResponse> {
    let name = query_param(query, "name").ok_or_else(|| anyhow!("missing ?name="))?;
    // db::table_rows validates the name against the DB's actual table list —
    // never raw identifiers from the client.
    let rows = crate::db::table_rows(&name, 50)?;
    // SysTrack tables store names as integer string-IDs; ship the id→string
    // map alongside so the dashboard can render human-readable rows.
    let resolved = crate::db::resolve_string_ids(&rows);
    Ok(json_response(
        &serde_json::json!({ "name": name, "rows": rows, "resolved": resolved }),
    ))
}

/// Substring search over SASTR/SASTRUSER — "has this endpoint ever seen X".
/// Zero matches is conclusive: nothing in the DB can reference a string that
/// isn't in the string tables.
fn api_db_strings(query: &str) -> Result<HttpResponse> {
    let q = query_param(query, "q").unwrap_or_default();
    if q.trim().len() < 2 {
        bail!("need ?q= of at least 2 characters");
    }
    let rows = crate::db::search_strings(q.trim(), 200)?;
    Ok(json_response(
        &serde_json::json!({ "q": q.trim(), "rows": rows, "limit": 200 }),
    ))
}

static COLLECT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a full `lsman collect` support bundle in a temp dir and stream the
/// .tar.gz. Read-only against the agent, so a plain GET link works; slow
/// (scans whole logs), but it only ties up one of the four worker threads.
fn api_collect(query: &str, handle: &tokio::runtime::Handle) -> Result<HttpResponse> {
    let since = query_param(query, "since").filter(|s| s != "all");
    if let Some(s) = &since {
        if crate::triage::parse_since(s).is_none() {
            bail!("bad since '{}' (use e.g. 30m, 24h, 7d)", s);
        }
    }
    let out_dir = std::env::temp_dir().join(format!(
        "lsman-collect-{}-{}",
        std::process::id(),
        COLLECT_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&out_dir)?;
    let result = handle.block_on(crate::collect::run(crate::collect::CollectOptions {
        output_dir: out_dir.clone(),
        max_log_mb: 50,
        since,
    }));
    let tarball = match result {
        Ok(t) => t,
        Err(e) => {
            fs::remove_dir_all(&out_dir).ok();
            return Err(e);
        }
    };
    let filename = tarball
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lsman-diag.tar.gz".to_string());
    let file = fs::File::open(&tarball)?;
    // Unlink now; the open handle keeps the bytes alive until streamed.
    fs::remove_dir_all(&out_dir).ok();
    Ok(attachment(tiny_http::Response::from_file(file), &filename))
}

/// Per-request read cap on a crash report; macOS .ips files are tens of KB.
const CRASH_READ_CAP: u64 = 512 * 1024;

#[derive(Serialize)]
struct CrashContent {
    name: String,
    path: String,
    size_bytes: u64,
    truncated: bool,
    content: String,
}

fn api_crash(query: &str) -> Result<HttpResponse> {
    let name = query_param(query, "name").ok_or_else(|| anyhow!("missing ?name="))?;
    // Resolve strictly against the listed crash reports (known dirs + known
    // file-name prefixes) — never an arbitrary path from the client.
    let path = report::list_crash_reports()
        .into_iter()
        .map(|(_, p)| p)
        .find(|p| p.file_name().is_some_and(|f| f.to_string_lossy() == name))
        .ok_or_else(|| anyhow!("unknown crash report '{}'", name))?;
    let mut file = fs::File::open(&path).map_err(|e| open_error(&path, e))?;
    let len = file.metadata()?.len();
    let mut buf = Vec::new();
    file.by_ref().take(CRASH_READ_CAP).read_to_end(&mut buf)?;
    Ok(json_response(&CrashContent {
        name,
        path: path.display().to_string(),
        size_bytes: len,
        truncated: len > CRASH_READ_CAP,
        content: String::from_utf8_lossy(&buf).into_owned(),
    }))
}

#[derive(Serialize)]
struct LogTail {
    name: String,
    path: String,
    size_bytes: u64,
    lines: Vec<String>,
    /// True when the tail window starts mid-file (only the last 1 MB is read).
    windowed: bool,
    /// Byte offset the next incremental (`?offset=`) request should use.
    /// Points past the last complete line returned.
    next_offset: u64,
    /// Incremental mode only: the file shrank or grew past the read cap —
    /// the client should do a fresh full tail instead of appending.
    reset: bool,
}

fn api_log(query: &str) -> Result<HttpResponse> {
    let name = query_param(query, "name").ok_or_else(|| anyhow!("missing ?name="))?;
    let log = paths::find_log(&name).ok_or_else(|| anyhow!("unknown log '{}'", name))?;
    let errors_only = query_param(query, "errors").as_deref() == Some("1");

    if let Some(offset) = query_param(query, "offset").and_then(|v| v.parse().ok()) {
        let (lines, size, next_offset, reset) = tail_since(&log.path, offset, errors_only)?;
        return Ok(json_response(&LogTail {
            name,
            path: log.path.display().to_string(),
            size_bytes: size,
            lines,
            windowed: false,
            next_offset,
            reset,
        }));
    }

    let n: usize = query_param(query, "n")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let n = n.min(MAX_TAIL_LINES);
    let (lines, size, windowed, next_offset) = tail_capped(&log.path, n, errors_only)?;
    Ok(json_response(&LogTail {
        name,
        path: log.path.display().to_string(),
        size_bytes: size,
        lines,
        windowed,
        next_offset,
        reset: false,
    }))
}

/// Incremental follow: complete lines appended since byte `offset`. A shrunken
/// file (rotation/truncation) or more than 1 MB of growth returns
/// `reset = true` so the caller re-tails from scratch. `next_offset` stops at
/// the last newline, so a partially-written final line is never split.
fn tail_since(path: &Path, offset: u64, errors_only: bool) -> Result<(Vec<String>, u64, u64, bool)> {
    let mut file = fs::File::open(path).map_err(|e| open_error(path, e))?;
    let len = file.metadata()?.len();
    if len < offset || len - offset > TAIL_READ_CAP {
        return Ok((Vec::new(), len, len, true));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::with_capacity((len - offset) as usize);
    file.read_to_end(&mut buf)?;
    // Hold back a trailing partial line for the next poll.
    let complete = match buf.iter().rposition(|&b| b == b'\n') {
        Some(pos) => pos + 1,
        None => 0,
    };
    let text = String::from_utf8_lossy(&buf[..complete]);
    let lines = text
        .lines()
        .filter(|l| !errors_only || triage::is_error_line(l))
        .map(|l| l.to_string())
        .collect();
    Ok((lines, len, offset + complete as u64, false))
}

/// Last `n` (optionally error-filtered) complete lines, reading at most the
/// final 1 MB of the file so polling stays cheap on huge logs. Also returns
/// the newline-aligned offset incremental follows should continue from (a
/// trailing partially-written line is left for the next poll).
fn tail_capped(path: &Path, n: usize, errors_only: bool) -> Result<(Vec<String>, u64, bool, u64)> {
    let mut file = fs::File::open(path).map_err(|e| open_error(path, e))?;
    let len = file.metadata()?.len();
    let windowed = len > TAIL_READ_CAP;
    let start = if windowed { len - TAIL_READ_CAP } else { 0 };
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity(TAIL_READ_CAP.min(len) as usize);
    file.read_to_end(&mut buf)?;
    let complete = match buf.iter().rposition(|&b| b == b'\n') {
        Some(pos) => pos + 1,
        None => buf.len(),
    };
    let next_offset = start + complete as u64;
    let text = String::from_utf8_lossy(&buf[..complete]);

    let mut lines: Vec<String> = text
        .lines()
        .skip(if windowed { 1 } else { 0 }) // first line is likely partial
        .filter(|l| !errors_only || triage::is_error_line(l))
        .map(|l| l.to_string())
        .collect();
    if lines.len() > n {
        lines.drain(..lines.len() - n);
    }
    Ok((lines, len, windowed, next_offset))
}

#[cfg(test)]
fn write_temp(content: &[u8]) -> PathBuf {
    use std::io::Write;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "lsman-tail-test-{}-{}.log",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::File::create(&p).unwrap().write_all(content).unwrap();
    p
}

fn open_error(path: &Path, e: std::io::Error) -> anyhow::Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied && unsafe { libc::geteuid() } != 0 {
        anyhow!(
            "permission denied reading {} — run lsman serve with sudo",
            path.display()
        )
    } else {
        anyhow!("reading {}: {}", path.display(), e)
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

/// Minimal %XX decoding so keys like `log%3Aagent` survive URL encoding.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

type HttpResponse = tiny_http::ResponseBox;

fn json_response<T: Serialize>(value: &T) -> HttpResponse {
    let body = serde_json::to_vec(value).unwrap_or_else(|e| {
        format!("{{\"error\":\"serialize: {}\"}}", e).into_bytes()
    });
    tiny_http::Response::from_data(body)
        .with_header(header("Content-Type", "application/json"))
        .boxed()
}

fn html_response(body: &str) -> HttpResponse {
    tiny_http::Response::from_data(body.as_bytes().to_vec())
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .boxed()
}

fn error_response(code: u16, msg: &str) -> HttpResponse {
    let body = serde_json::json!({ "error": msg });
    tiny_http::Response::from_data(serde_json::to_vec(&body).unwrap_or_default())
        .with_status_code(code)
        .with_header(header("Content-Type", "application/json"))
        .boxed()
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static header is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_targets_resolve_known_keys_only() {
        let (path, name) = download_target("cfg").unwrap();
        assert!(path.ends_with("lsiagent.cfg"));
        assert_eq!(name, "lsiagent.cfg");

        let (path, name) = download_target("db").unwrap();
        assert!(path.ends_with("collect.sqlite3"));
        assert_eq!(name, "collect.sqlite3");

        let (path, name) = download_target("profile").unwrap();
        assert!(path.ends_with("profile"));
        assert_eq!(name, "profile");

        // "webcom" is a known log on every platform
        let (path, name) = download_target("log:webcom").unwrap();
        assert!(path.ends_with("lsiwebcom.log"));
        assert_eq!(name, "lsiwebcom.log");

        assert!(download_target("passwd").is_none());
        assert!(download_target("log:../../etc/passwd").is_none());
        assert!(download_target("log:").is_none());
    }

    #[test]
    fn incremental_tail_returns_only_complete_new_lines() {
        let p = write_temp(b"one\ntwo\npart");
        // full tail: partial trailing line is held back, offset stops at \n
        let (lines, len, windowed, next) = tail_capped(&p, 100, false).unwrap();
        assert_eq!(lines, ["one", "two"]);
        assert_eq!(len, 12);
        assert!(!windowed);
        assert_eq!(next, 8);

        // nothing new yet: "part" is still unterminated
        let (lines, _, next2, reset) = tail_since(&p, next, false).unwrap();
        assert!(lines.is_empty() && !reset);
        assert_eq!(next2, 8);

        // finish the line and append another
        fs::write(&p, b"one\ntwo\npart done\nthree\n").unwrap();
        let (lines, _, next3, reset) = tail_since(&p, next, false).unwrap();
        assert_eq!(lines, ["part done", "three"]);
        assert!(!reset);
        assert_eq!(next3, 24);

        // rotation: file shrank below our offset → reset
        fs::write(&p, b"new\n").unwrap();
        let (lines, _, _, reset) = tail_since(&p, next3, false).unwrap();
        assert!(lines.is_empty() && reset);

        fs::remove_file(&p).ok();
    }

    #[test]
    fn incremental_tail_filters_errors() {
        let p = write_temp(b"07-04 12:00:00 a(1) -I fine\n07-04 12:00:01 b(2) -E broken\n");
        let (lines, _, _, reset) = tail_since(&p, 0, true).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("-E broken") && !reset);
        fs::remove_file(&p).ok();
    }

    #[test]
    fn query_params_are_percent_decoded() {
        assert_eq!(
            query_param("file=log%3Awebcom", "file").as_deref(),
            Some("log:webcom")
        );
        assert_eq!(query_param("since=24h&n=200", "n").as_deref(), Some("200"));
        // malformed escapes pass through untouched
        assert_eq!(percent_decode("a%zz%4"), "a%zz%4");
    }
}
