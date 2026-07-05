//! One-shot diagnostic snapshot of the agent install: everything a customer
//! case / COL ticket usually needs, gathered once and rendered as markdown
//! (`lsman report`), bundled (`lsman collect`), or served as JSON (`lsman serve`).

use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Utc;
use serde::Serialize;

use crate::daemon::{DaemonManager, DaemonState};
use crate::network;
use crate::paths;
use crate::triage::{self, ErrorGroup, ErrorGrouper};

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub exists: bool,
    pub is_dir: bool,
    pub size_bytes: u64,
    /// Seconds since last modification; None when missing/unreadable.
    pub modified_secs_ago: Option<u64>,
}

impl FileInfo {
    pub fn for_path(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(meta) => Self {
                path: path.display().to_string(),
                exists: true,
                is_dir: meta.is_dir(),
                size_bytes: meta.len(),
                modified_secs_ago: meta
                    .modified()
                    .ok()
                    .and_then(|m| SystemTime::now().duration_since(m).ok())
                    .map(|d| d.as_secs()),
            },
            Err(_) => Self {
                path: path.display().to_string(),
                exists: false,
                is_dir: false,
                size_bytes: 0,
                modified_secs_ago: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonInfo {
    pub state: String,
    pub pid: Option<u32>,
    pub service_manager: String,
    /// False when the kernel refused stats (non-root vs root daemon).
    pub stats_available: bool,
    pub cpu_percent: f32,
    pub memory_kb: u64,
    pub uptime_seconds: u64,
    pub open_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigInfo {
    pub file: FileInfo,
    /// `[Debug] LogLevel2` — None when unset (agent uses master config / info).
    pub trace_level: Option<i64>,
    pub trace_level_name: Option<String>,
    pub read_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UplinkConn {
    pub local: String,
    pub remote: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UplinkInfo {
    /// False when lsof couldn't see the daemon's sockets (needs sudo).
    pub available: bool,
    pub established: Vec<UplinkConn>,
    pub total_sockets: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogInfo {
    pub name: String,
    pub description: String,
    pub file: FileInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashInfo {
    pub path: String,
    /// Local time, for display.
    pub modified: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorScan {
    /// The `--since` window used, e.g. "24h"; None = whole logs.
    pub since: Option<String>,
    pub total_matches: usize,
    pub groups: Vec<ErrorGroup>,
    /// Logs that exist but couldn't be read (permissions).
    pub unreadable_logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagSnapshot {
    pub generated_utc: String,
    pub hostname: String,
    pub platform: String,
    pub lsman_version: String,
    pub running_as_root: bool,
    pub daemon: DaemonInfo,
    pub binary: FileInfo,
    pub base_dir: FileInfo,
    pub config: ConfigInfo,
    pub database: FileInfo,
    pub uplink: UplinkInfo,
    pub logs: Vec<LogInfo>,
    pub crashes: Vec<CrashInfo>,
    /// Present when an error scan was requested (it reads whole logs, so it's
    /// optional — the web UI fetches it separately from the cheap snapshot).
    pub errors: Option<ErrorScan>,
}

/// Gather the cheap parts of the snapshot (no log scanning).
pub async fn gather(mgr: &mut DaemonManager) -> DiagSnapshot {
    mgr.update_status().await;
    // sysinfo needs two samples spaced apart for a meaningful CPU number
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    mgr.update_status().await;

    let running_as_root = unsafe { libc::geteuid() } == 0;
    let state = mgr.get_state();
    let stats = mgr.get_process_stats();

    let daemon = DaemonInfo {
        state: state.to_string(),
        pid: stats.pid,
        service_manager: mgr.manager().name().to_lowercase(),
        stats_available: !(state == DaemonState::Running
            && stats.memory_usage == 0
            && !running_as_root),
        cpu_percent: stats.cpu_usage,
        memory_kb: stats.memory_usage,
        uptime_seconds: stats.uptime_seconds,
        open_files: stats.open_files,
    };

    let cfg_path = paths::config_path();
    let config = match fs::read_to_string(&cfg_path) {
        Ok(content) => {
            let level = crate::cli::read_log_level(&content);
            ConfigInfo {
                file: FileInfo::for_path(&cfg_path),
                trace_level: level,
                trace_level_name: level.map(|l| crate::cli::level_name(l).to_string()),
                read_error: None,
            }
        }
        Err(e) => ConfigInfo {
            file: FileInfo::for_path(&cfg_path),
            trace_level: None,
            trace_level_name: None,
            read_error: Some(e.to_string()),
        },
    };

    let uplink = match stats.pid {
        Some(pid) => match network::query_connections(pid) {
            Ok(conns) => UplinkInfo {
                available: !conns.is_empty() || running_as_root,
                established: conns
                    .iter()
                    .filter(|c| c.is_established())
                    .map(|c| UplinkConn {
                        local: c.local.clone(),
                        remote: c.remote.clone().unwrap_or_default(),
                    })
                    .collect(),
                total_sockets: conns.len(),
                error: None,
            },
            Err(e) => UplinkInfo {
                available: false,
                established: Vec::new(),
                total_sockets: 0,
                error: Some(e.to_string()),
            },
        },
        None => UplinkInfo {
            available: false,
            established: Vec::new(),
            total_sockets: 0,
            error: Some("daemon not running".to_string()),
        },
    };

    let logs = paths::known_logs()
        .into_iter()
        .map(|l| LogInfo {
            name: l.name,
            description: l.description.to_string(),
            file: FileInfo::for_path(&l.path),
        })
        .collect();

    let crashes = list_crash_reports()
        .into_iter()
        .map(|(mtime, path)| {
            let when: chrono::DateTime<chrono::Local> = mtime.into();
            CrashInfo {
                path: path.display().to_string(),
                modified: when.format("%Y-%m-%d %H:%M:%S").to_string(),
            }
        })
        .collect();

    DiagSnapshot {
        generated_utc: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        hostname: sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string()),
        platform: std::env::consts::OS.to_string(),
        lsman_version: env!("CARGO_PKG_VERSION").to_string(),
        running_as_root,
        daemon,
        binary: FileInfo::for_path(&paths::daemon_binary()),
        base_dir: FileInfo::for_path(&paths::base_dir()),
        config,
        database: FileInfo::for_path(&paths::database_path()),
        uplink,
        logs,
        crashes,
        errors: None,
    }
}

/// Agent-related crash reports, newest first.
pub fn list_crash_reports() -> Vec<(SystemTime, PathBuf)> {
    let mut reports: Vec<(SystemTime, PathBuf)> = Vec::new();
    for dir in paths::crash_report_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().into_owned();
            if paths::CRASH_REPORT_PREFIXES.iter().any(|p| fname.starts_with(p)) {
                if let Ok(meta) = entry.metadata() {
                    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    reports.push((mtime, entry.path()));
                }
            }
        }
    }
    reports.sort_by(|a, b| b.0.cmp(&a.0));
    reports
}

/// Stream every existing known log and group error lines by source site.
/// `since` filters by the line's own UTC timestamp; unstamped lines are only
/// included when no window is set.
pub fn scan_errors(since: Option<chrono::Duration>, since_label: Option<String>) -> ErrorScan {
    scan_errors_in(&paths::known_logs(), since, since_label)
}

/// Like [`scan_errors`], restricted to the given logs.
pub fn scan_errors_in(
    logs: &[paths::KnownLog],
    since: Option<chrono::Duration>,
    since_label: Option<String>,
) -> ErrorScan {
    let now = Utc::now();
    let cutoff = since.map(|d| now.naive_utc() - d);
    let mut grouper = ErrorGrouper::new();
    let mut unreadable = Vec::new();

    for log in logs {
        if !log.path.exists() {
            continue;
        }
        let file = match fs::File::open(&log.path) {
            Ok(f) => f,
            Err(_) => {
                unreadable.push(log.name.clone());
                continue;
            }
        };
        // Stream: agent logs can be hundreds of MB
        let reader = std::io::BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            if !triage::is_error_line(&line) {
                continue;
            }
            if let Some(cutoff) = cutoff {
                match triage::parse_line(&line, now).timestamp {
                    Some(ts) if ts >= cutoff => {}
                    _ => continue,
                }
            }
            grouper.add(&log.name, &line, now);
        }
    }

    ErrorScan {
        since: since_label,
        total_matches: grouper.total(),
        groups: grouper.into_sorted(),
        unreadable_logs: unreadable,
    }
}

/// Render the snapshot as ticket-pasteable markdown.
pub fn to_markdown(snap: &DiagSnapshot) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    let w = &mut md;

    let _ = writeln!(w, "# LSI agent diagnostic report — {}", snap.hostname);
    let _ = writeln!(
        w,
        "\nGenerated {} by lsman {} on {}{}",
        snap.generated_utc,
        snap.lsman_version,
        snap.platform,
        if snap.running_as_root { "" } else { " (NOT run as root — some data unavailable)" }
    );

    let _ = writeln!(w, "\n## Daemon\n");
    match snap.daemon.pid {
        Some(pid) => {
            let _ = writeln!(
                w,
                "- state: **{}** (pid {}) via {}",
                snap.daemon.state, pid, snap.daemon.service_manager
            );
        }
        None => {
            let _ = writeln!(
                w,
                "- state: **{}** (process not found) via {}",
                snap.daemon.state, snap.daemon.service_manager
            );
        }
    }
    if snap.daemon.stats_available && snap.daemon.pid.is_some() {
        let _ = writeln!(
            w,
            "- cpu/mem: {:.1}% / {} | uptime: {} | open files: {}",
            snap.daemon.cpu_percent,
            crate::cli::format_kb(snap.daemon.memory_kb),
            crate::cli::format_duration(snap.daemon.uptime_seconds),
            snap.daemon.open_files
        );
    } else if snap.daemon.pid.is_some() {
        let _ = writeln!(w, "- cpu/mem: unavailable (daemon runs as root — rerun with sudo)");
    }

    let _ = writeln!(w, "\n## Install\n");
    let _ = writeln!(w, "- binary: {}", file_line(&snap.binary));
    let _ = writeln!(w, "- base dir: {}", file_line(&snap.base_dir));
    let trace = match snap.config.trace_level {
        Some(l) => format!(
            "trace level {} ({})",
            l,
            snap.config.trace_level_name.as_deref().unwrap_or("?")
        ),
        None => match &snap.config.read_error {
            Some(e) => format!("unreadable: {}", e),
            None => "trace level not set locally (master config / default 3=info)".to_string(),
        },
    };
    let _ = writeln!(w, "- config: {} — {}", file_line(&snap.config.file), trace);
    let _ = writeln!(w, "- database: {}", file_line(&snap.database));

    let _ = writeln!(w, "\n## Uplink to master\n");
    if !snap.uplink.available {
        let _ = writeln!(
            w,
            "- unavailable ({})",
            snap.uplink.error.as_deref().unwrap_or("daemon sockets not visible — rerun with sudo")
        );
    } else if snap.uplink.established.is_empty() {
        let _ = writeln!(
            w,
            "- **no established connections** ({} open sockets)",
            snap.uplink.total_sockets
        );
    } else {
        for c in &snap.uplink.established {
            let _ = writeln!(w, "- ESTABLISHED {} -> {}", c.local, c.remote);
        }
    }

    let _ = writeln!(w, "\n## Logs (timestamps inside logs are UTC)\n");
    let _ = writeln!(w, "| log | size | last written |");
    let _ = writeln!(w, "|---|---|---|");
    for log in &snap.logs {
        if !log.file.exists {
            continue;
        }
        let _ = writeln!(
            w,
            "| {} | {} | {} |",
            log.name,
            crate::cli::format_bytes(log.file.size_bytes),
            log.file
                .modified_secs_ago
                .map(|s| format!("{} ago", crate::cli::format_duration(s)))
                .unwrap_or_else(|| "?".to_string())
        );
    }

    if let Some(errors) = &snap.errors {
        let window = errors
            .since
            .as_deref()
            .map(|s| format!(" (last {})", s))
            .unwrap_or_default();
        let _ = writeln!(w, "\n## Error triage{}\n", window);
        if errors.groups.is_empty() {
            let _ = writeln!(w, "No error/crash lines found.");
        } else {
            let _ = writeln!(
                w,
                "{} matching lines across {} sites. `<file>(<line>)` points into the agent source.\n",
                errors.total_matches,
                errors.groups.len()
            );
            let _ = writeln!(w, "| log | site | count | first seen | last seen |");
            let _ = writeln!(w, "|---|---|---|---|---|");
            for g in &errors.groups {
                let _ = writeln!(
                    w,
                    "| {} | `{}` | {} | {} | {} |",
                    g.log,
                    g.site,
                    g.count,
                    g.first_seen.as_deref().unwrap_or("-"),
                    g.last_seen.as_deref().unwrap_or("-")
                );
            }
            let _ = writeln!(w, "\nMost recent line per site:\n");
            for g in &errors.groups {
                let _ = writeln!(w, "```\n{}\n```", g.sample);
            }
        }
        if !errors.unreadable_logs.is_empty() {
            let _ = writeln!(
                w,
                "\nUnreadable logs (permissions): {}",
                errors.unreadable_logs.join(", ")
            );
        }
    }

    let _ = writeln!(w, "\n## Crash reports\n");
    if snap.crashes.is_empty() {
        let _ = writeln!(
            w,
            "None found{}.",
            if snap.running_as_root { "" } else { " (daemon reports may need sudo)" }
        );
    } else {
        for c in snap.crashes.iter().take(10) {
            let _ = writeln!(w, "- {} — {}", c.modified, c.path);
        }
        if snap.crashes.len() > 10 {
            let _ = writeln!(w, "- … and {} more", snap.crashes.len() - 10);
        }
    }

    md
}

fn file_line(f: &FileInfo) -> String {
    if !f.exists {
        return format!("{} (missing)", f.path);
    }
    let size = if f.is_dir {
        "dir".to_string()
    } else {
        crate::cli::format_bytes(f.size_bytes)
    };
    let age = f
        .modified_secs_ago
        .map(|s| format!(", modified {} ago", crate::cli::format_duration(s)))
        .unwrap_or_default();
    format!("{} ({}{})", f.path, size, age)
}
