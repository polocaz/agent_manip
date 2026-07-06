//! `lsman serve` — a local, read-only web dashboard over the same data the
//! CLI/TUI expose: daemon status, uplink, error triage, log tails. Useful for
//! watching an agent while reproducing a customer issue, or from a browser
//! when the box only has SSH + port forward.
//!
//! Read-only by design: no daemon control, no config writes. Binds 127.0.0.1
//! unless told otherwise.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
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
        println!("warning: bound to {} — the dashboard is unauthenticated; prefer 127.0.0.1 + SSH port forward", opts.bind);
    }
    if unsafe { libc::geteuid() } != 0 {
        println!("note: not running as root — daemon stats/sockets and some logs will be unavailable (use sudo)");
    }
    println!("read-only dashboard; Ctrl-C to stop");

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
    request: tiny_http::Request,
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
        _ => error_response(404, "not found"),
    };

    let _ = request.respond(response);
}

#[derive(Serialize)]
struct LogTail {
    name: String,
    path: String,
    size_bytes: u64,
    lines: Vec<String>,
    /// True when the tail window starts mid-file (only the last 1 MB is read).
    windowed: bool,
}

fn api_log(query: &str) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let name = query_param(query, "name").ok_or_else(|| anyhow!("missing ?name="))?;
    let log = paths::find_log(&name).ok_or_else(|| anyhow!("unknown log '{}'", name))?;
    let n: usize = query_param(query, "n")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let n = n.min(MAX_TAIL_LINES);
    let errors_only = query_param(query, "errors").as_deref() == Some("1");

    let (lines, size, windowed) = tail_capped(&log.path, n, errors_only)?;
    Ok(json_response(&LogTail {
        name,
        path: log.path.display().to_string(),
        size_bytes: size,
        lines,
        windowed,
    }))
}

/// Last `n` (optionally error-filtered) lines, reading at most the final
/// 1 MB of the file so polling stays cheap on huge logs.
fn tail_capped(path: &Path, n: usize, errors_only: bool) -> Result<(Vec<String>, u64, bool)> {
    let mut file = fs::File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied && unsafe { libc::geteuid() } != 0 {
            anyhow!("permission denied reading {} — run lsman serve with sudo", path.display())
        } else {
            anyhow!("reading {}: {}", path.display(), e)
        }
    })?;
    let len = file.metadata()?.len();
    let windowed = len > TAIL_READ_CAP;
    if windowed {
        file.seek(SeekFrom::Start(len - TAIL_READ_CAP))?;
    }
    let mut buf = Vec::with_capacity(TAIL_READ_CAP.min(len) as usize);
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);

    let mut lines: Vec<String> = text
        .lines()
        .skip(if windowed { 1 } else { 0 }) // first line is likely partial
        .filter(|l| !errors_only || triage::is_error_line(l))
        .map(|l| l.to_string())
        .collect();
    if lines.len() > n {
        lines.drain(..lines.len() - n);
    }
    Ok((lines, len, windowed))
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

type HttpResponse = tiny_http::Response<std::io::Cursor<Vec<u8>>>;

fn json_response<T: Serialize>(value: &T) -> HttpResponse {
    let body = serde_json::to_vec(value).unwrap_or_else(|e| {
        format!("{{\"error\":\"serialize: {}\"}}", e).into_bytes()
    });
    tiny_http::Response::from_data(body).with_header(header("Content-Type", "application/json"))
}

fn html_response(body: &str) -> HttpResponse {
    tiny_http::Response::from_data(body.as_bytes().to_vec())
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
}

fn error_response(code: u16, msg: &str) -> HttpResponse {
    let body = serde_json::json!({ "error": msg });
    tiny_http::Response::from_data(serde_json::to_vec(&body).unwrap_or_default())
        .with_status_code(code)
        .with_header(header("Content-Type", "application/json"))
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static header is valid")
}
