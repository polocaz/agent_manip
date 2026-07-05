//! Non-interactive subcommands for debugging the agent from the command line.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, Result};
use clap::Subcommand;

use crate::daemon::{DaemonManager, DaemonState};
use crate::network;
use crate::paths;

#[derive(Subcommand)]
pub enum CliCommand {
    /// Show daemon status: state, resources, uplink, config, log freshness
    Status,
    /// List the agent's log files, or print/tail one (names from `lsman logs`)
    Logs {
        /// Log name (e.g. agent, webcom, sensors, slow-sql, inventory, statusbar)
        name: Option<String>,
        /// Number of lines to show from the end of the log
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Keep the log open and print new lines as they arrive
        #[arg(short, long)]
        follow: bool,
        /// Only show error/crash lines
        #[arg(short, long)]
        errors: bool,
        /// Print the log file path and exit
        #[arg(long)]
        path: bool,
    },
    /// Triage scan: find error/crash lines across the agent logs
    Errors {
        /// Restrict to one log name (default: all main agent logs)
        #[arg(long)]
        log: Option<String>,
        /// Max matching lines to show per log file
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
    /// Print the agent config file (lsiagent.cfg)
    Config {
        /// Print the config file path and exit
        #[arg(long)]
        path: bool,
    },
    /// Show or set the agent trace level ([Debug] LogLevel2 in lsiagent.cfg)
    Trace {
        /// New level: 0-8, or none/error/warning/info/trace1..trace5
        level: Option<String>,
        /// Restart the daemon after changing the level so it takes effect now
        #[arg(long)]
        restart: bool,
    },
    /// Inspect the agent SQLite database read-only (no arg: list tables)
    Db {
        /// SQL to run, e.g. `lsman db "SELECT count(*) FROM SYSTEM"`
        sql: Option<String>,
    },
    /// List recent agent crash reports (macOS DiagnosticReports)
    Crashes {
        /// Max reports to list
        #[arg(short = 'n', long, default_value_t = 10)]
        count: usize,
        /// Print a report: an index from the list, or "latest"
        #[arg(long)]
        show: Option<String>,
    },
    /// Show the daemon's open network connections
    Net,
    /// Start the daemon (launchctl / systemctl)
    Start,
    /// Stop the daemon (launchctl bootout / systemctl stop)
    Stop,
    /// Restart the daemon
    Restart,
}

pub async fn run(command: CliCommand) -> Result<()> {
    match command {
        CliCommand::Status => cmd_status().await,
        CliCommand::Logs { name, lines, follow, errors, path } => {
            cmd_logs(name, lines, follow, errors, path)
        }
        CliCommand::Errors { log, lines } => cmd_errors(log, lines),
        CliCommand::Config { path } => cmd_config(path),
        CliCommand::Trace { level, restart } => cmd_trace(level, restart).await,
        CliCommand::Db { sql } => cmd_db(sql),
        CliCommand::Crashes { count, show } => cmd_crashes(count, show),
        CliCommand::Net => cmd_net().await,
        CliCommand::Start => cmd_daemon_control(DaemonAction::Start).await,
        CliCommand::Stop => cmd_daemon_control(DaemonAction::Stop).await,
        CliCommand::Restart => cmd_daemon_control(DaemonAction::Restart).await,
    }
}

// ---------------------------------------------------------------------------
// status

async fn cmd_status() -> Result<()> {
    let mut mgr = DaemonManager::new()?;
    mgr.update_status().await;
    // sysinfo needs two samples spaced apart for a meaningful CPU number
    tokio::time::sleep(Duration::from_millis(250)).await;
    mgr.update_status().await;

    let state = mgr.get_state();
    let stats = mgr.get_process_stats();

    println!("LSI agent status");
    match stats.pid {
        Some(pid) => println!(
            "  daemon:    {} (pid {}) via {}",
            state,
            pid,
            mgr.manager().name().to_lowercase()
        ),
        None => println!(
            "  daemon:    {} (process '{}' not found) via {}",
            state,
            mgr.get_process_name(),
            mgr.manager().name().to_lowercase()
        ),
    }
    if state == DaemonState::Running {
        // macOS won't let a non-root process read a root daemon's stats
        if stats.memory_usage == 0 && unsafe { libc::geteuid() } != 0 {
            println!("  cpu/mem:   unavailable (daemon runs as root — use sudo)");
        } else {
            println!(
                "  cpu/mem:   {:.1}% / {}",
                stats.cpu_usage,
                format_kb(stats.memory_usage)
            );
            println!(
                "  uptime:    {} | open files: {}",
                if stats.start_time > 0 {
                    format_duration(stats.uptime_seconds)
                } else {
                    "unknown".to_string()
                },
                stats.open_files
            );
        }
    }
    println!("  binary:    {}", describe_file(&paths::daemon_binary()));
    println!("  base dir:  {}", describe_file(&paths::base_dir()));

    // Config + trace level
    let cfg_path = paths::config_path();
    match fs::read_to_string(&cfg_path) {
        Ok(content) => {
            let level = read_log_level(&content);
            println!(
                "  config:    {} (trace level: {})",
                cfg_path.display(),
                level.map_or("not set (default 3/info)".to_string(), |l| format!(
                    "{} ({})",
                    l,
                    level_name(l)
                ))
            );
        }
        Err(e) => println!("  config:    {} ({})", cfg_path.display(), e),
    }

    println!("  database:  {}", describe_file(&paths::database_path()));

    // Uplink
    if let Some(pid) = stats.pid {
        match network::query_connections(pid) {
            Ok(conns) => {
                let established: Vec<_> =
                    conns.iter().filter(|c| c.is_established()).collect();
                if conns.is_empty() && unsafe { libc::geteuid() } != 0 {
                    // lsof shows nothing for another user's process
                    println!("  uplink:    unavailable (daemon runs as root — use sudo)");
                } else if established.is_empty() {
                    println!("  uplink:    no established connections");
                } else {
                    let remotes: Vec<String> = established
                        .iter()
                        .filter_map(|c| c.remote.clone())
                        .collect();
                    println!(
                        "  uplink:    {} established ({})",
                        established.len(),
                        remotes.join(", ")
                    );
                }
            }
            Err(e) => println!("  uplink:    unavailable ({})", e),
        }
    }

    println!("  logs:      (timestamps inside logs are UTC)");
    for log in paths::known_logs() {
        println!("    {:<12} {}", log.name, describe_file(&log.path));
    }

    if unsafe { libc::geteuid() } != 0 {
        println!("\n  note: not running as root — some files above may be unreadable (try sudo)");
    }
    Ok(())
}

/// "12.3 MB, modified 5s ago" / "missing" for a path.
fn describe_file(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(meta) => {
            let size = if meta.is_dir() {
                "dir".to_string()
            } else {
                format_bytes(meta.len())
            };
            let age = meta
                .modified()
                .ok()
                .and_then(|m| SystemTime::now().duration_since(m).ok())
                .map(|d| format!(", modified {} ago", format_duration(d.as_secs())))
                .unwrap_or_default();
            format!("{} ({}{})", path.display(), size, age)
        }
        Err(_) => format!("{} (missing)", path.display()),
    }
}

// ---------------------------------------------------------------------------
// logs

fn cmd_logs(
    name: Option<String>,
    lines: usize,
    follow: bool,
    errors_only: bool,
    path_only: bool,
) -> Result<()> {
    let Some(name) = name else {
        println!("{:<12} {:<38} DESCRIPTION", "NAME", "STATUS");
        for log in paths::known_logs() {
            let status = match fs::metadata(&log.path) {
                Ok(m) => {
                    let age = m
                        .modified()
                        .ok()
                        .and_then(|t| SystemTime::now().duration_since(t).ok())
                        .map(|d| format_duration(d.as_secs()))
                        .unwrap_or_else(|| "?".into());
                    format!("{}, modified {} ago", format_bytes(m.len()), age)
                }
                Err(_) => "missing".to_string(),
            };
            println!("{:<12} {:<38} {}", log.name, status, log.description);
        }
        println!("\nusage: lsman logs <name> [-n LINES] [-f] [-e]   (log timestamps are UTC)");
        return Ok(());
    };

    let log = paths::find_log(&name)
        .ok_or_else(|| anyhow!("unknown log '{}' — run `lsman logs` to list names", name))?;

    if path_only {
        println!("{}", log.path.display());
        return Ok(());
    }

    for line in tail_lines(&log.path, lines, errors_only)? {
        println!("{}", line);
    }

    if follow {
        follow_log(&log.path, errors_only)?;
    }
    Ok(())
}

/// Last `n` (optionally error-filtered) lines of a file, streaming so a
/// multi-hundred-MB agent log never gets loaded into memory at once.
fn tail_lines(path: &Path, n: usize, errors_only: bool) -> Result<Vec<String>> {
    use std::collections::VecDeque;
    use std::io::BufRead;

    let file = fs::File::open(path).map_err(|e| map_perm_error(e, path))?;
    let reader = std::io::BufReader::new(file);
    let mut last: VecDeque<String> = VecDeque::with_capacity(n + 1);
    for line in reader.lines() {
        let line = line.unwrap_or_else(|_| "<unreadable line>".to_string());
        if errors_only && !is_error_line(&line) {
            continue;
        }
        if last.len() == n {
            last.pop_front();
        }
        last.push_back(line);
    }
    Ok(last.into())
}

/// tail -f style follow, handling truncation/rotation.
fn follow_log(path: &Path, errors_only: bool) -> Result<()> {
    let mut pos = fs::metadata(path)?.len();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let len = match fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => continue, // rotated away; wait for it to come back
        };
        if len < pos {
            println!("--- log truncated/rotated ---");
            pos = 0;
        }
        if len > pos {
            let mut file = fs::File::open(path)?;
            file.seek(SeekFrom::Start(pos))?;
            let mut new_data = String::new();
            file.read_to_string(&mut new_data)?;
            pos = len;
            for line in new_data.lines() {
                if !errors_only || is_error_line(line) {
                    println!("{}", line);
                }
            }
            std::io::stdout().flush().ok();
        }
    }
}

// ---------------------------------------------------------------------------
// errors

/// Matches the first-pass triage pattern from the agent bug-investigation
/// runbook: explicit error-level lines (" -E ") plus crash-ish keywords.
fn is_error_line(line: &str) -> bool {
    if line.contains(" -E ") {
        return true;
    }
    let lower = line.to_lowercase();
    ["crash", "abort", "exception", "failed"]
        .iter()
        .any(|kw| lower.contains(kw))
}

fn cmd_errors(log_name: Option<String>, max_lines: usize) -> Result<()> {
    let logs: Vec<paths::KnownLog> = match log_name {
        Some(name) => vec![paths::find_log(&name)
            .ok_or_else(|| anyhow!("unknown log '{}' — run `lsman logs` to list names", name))?],
        // Default to the main daemon log(s): agent / agent1..5
        None => paths::known_logs()
            .into_iter()
            .filter(|l| l.name.starts_with("agent"))
            .collect(),
    };

    println!("(log timestamps are UTC; <file>(<line>) in each entry points into the agent source)\n");
    let mut total = 0usize;
    for log in logs {
        if !log.path.exists() {
            continue;
        }

        // Stream the file: agent logs can be hundreds of MB
        use std::collections::VecDeque;
        use std::io::BufRead;
        let file = fs::File::open(&log.path).map_err(|e| map_perm_error(e, &log.path))?;
        let reader = std::io::BufReader::new(file);
        let mut matches: VecDeque<(usize, String)> = VecDeque::with_capacity(max_lines + 1);
        let mut match_count = 0usize;
        for (line_no, line) in reader.lines().enumerate() {
            let Ok(line) = line else { continue };
            if is_error_line(&line) {
                match_count += 1;
                if matches.len() == max_lines {
                    matches.pop_front();
                }
                matches.push_back((line_no + 1, line));
            }
        }
        total += match_count;

        if matches.is_empty() {
            continue;
        }
        if match_count > max_lines {
            println!(
                "== {} ({} matches, showing last {}) ==",
                log.name, match_count, max_lines
            );
        } else {
            println!("== {} ({} matches) ==", log.name, match_count);
        }
        for (line_no, line) in &matches {
            println!("{}:{}: {}", log.name, line_no, line);
        }
        println!();
    }

    if total == 0 {
        println!("no error/crash lines found");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// config

fn cmd_config(path_only: bool) -> Result<()> {
    let cfg = paths::config_path();
    if path_only {
        println!("{}", cfg.display());
        return Ok(());
    }
    print!("{}", read_with_sudo_hint(&cfg)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// trace

const LEVEL_NAMES: &[(&str, i64)] = &[
    ("none", 0),
    ("error", 1),
    ("warning", 2),
    ("info", 3),
    ("trace1", 4),
    ("trace2", 5),
    ("trace3", 6),
    ("trace4", 7),
    ("trace5", 8),
];

fn level_name(level: i64) -> &'static str {
    LEVEL_NAMES
        .iter()
        .find(|(_, v)| *v == level)
        .map(|(n, _)| *n)
        .unwrap_or("?")
}

fn parse_level(s: &str) -> Result<i64> {
    if let Ok(n) = s.parse::<i64>() {
        if (0..=8).contains(&n) {
            return Ok(n);
        }
        bail!("level must be 0-8");
    }
    LEVEL_NAMES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(s))
        .map(|(_, v)| *v)
        .ok_or_else(|| anyhow!("unknown level '{}' (use 0-8 or none/error/warning/info/trace1..trace5)", s))
}

/// Read `LogLevel2` from the `[Debug]` section of lsiagent.cfg content.
fn read_log_level(cfg: &str) -> Option<i64> {
    let mut in_debug = false;
    for line in cfg.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_debug = trimmed.eq_ignore_ascii_case("[debug]");
            continue;
        }
        if in_debug {
            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim().eq_ignore_ascii_case("loglevel2") {
                    return value.trim().parse::<i64>().ok();
                }
            }
        }
    }
    None
}

/// Return cfg content with `LogLevel2=<level>` set in the `[Debug]` section,
/// creating the key or section as needed. Preserves everything else.
fn set_log_level(cfg: &str, level: i64) -> String {
    let mut lines: Vec<String> = cfg.lines().map(|l| l.to_string()).collect();
    let mut in_debug = false;
    let mut debug_header_idx: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_debug = trimmed.eq_ignore_ascii_case("[debug]");
            if in_debug {
                debug_header_idx = Some(i);
            }
            continue;
        }
        if in_debug {
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim().eq_ignore_ascii_case("loglevel2") {
                    lines[i] = format!("LogLevel2={}", level);
                    return join_lines(&lines);
                }
            }
        }
    }

    match debug_header_idx {
        Some(idx) => lines.insert(idx + 1, format!("LogLevel2={}", level)),
        None => {
            if !lines.is_empty() && !lines.last().is_none_or(|l| l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push("[Debug]".to_string());
            lines.push(format!("LogLevel2={}", level));
        }
    }
    join_lines(&lines)
}

fn join_lines(lines: &[String]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

async fn cmd_trace(level: Option<String>, restart: bool) -> Result<()> {
    let cfg_path = paths::config_path();

    let Some(level) = level else {
        // Show current level
        let content = read_with_sudo_hint(&cfg_path)?;
        let current = read_log_level(&content);
        match current {
            Some(l) => println!("current trace level: {} ({})", l, level_name(l)),
            None => println!("LogLevel2 not set locally — agent uses master config / default 3 (info)"),
        }
        println!("\nlevels: 0=none 1=error 2=warning 3=info(default) 4-8=trace1..trace5");
        println!("set with: lsman trace <level> [--restart]");
        return Ok(());
    };

    let level = parse_level(&level)?;
    let content = match fs::read_to_string(&cfg_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(map_perm_error(e, &cfg_path)),
    };

    let new_content = set_log_level(&content, level);
    fs::write(&cfg_path, new_content).map_err(|e| map_perm_error(e, &cfg_path))?;
    println!(
        "LogLevel2={} ({}) written to {}",
        level,
        level_name(level),
        cfg_path.display()
    );
    println!("note: a non-zero local LogLevel2 overrides the master-pushed config");

    if restart {
        let mut mgr = DaemonManager::new()?;
        mgr.update_status().await;
        mgr.restart_daemon()?;
        println!("daemon restarted");
    } else {
        println!("the daemon picks this up on its next config read; use --restart (or `lsman restart`) to apply now");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// db

fn cmd_db(sql: Option<String>) -> Result<()> {
    let db = paths::database_path();
    if !db.exists() {
        bail!("database not found at {}", db.display());
    }

    let sql = sql.unwrap_or_else(|| {
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;".to_string()
    });

    // -readonly so we can never corrupt a live agent database
    let status = Command::new("sqlite3")
        .arg("-readonly")
        .arg("-header")
        .arg("-column")
        .arg(&db)
        .arg(&sql)
        .status()
        .map_err(|e| anyhow!("failed to run sqlite3 (is it installed?): {}", e))?;

    if !status.success() {
        let hint = if unsafe { libc::geteuid() } != 0 {
            " — the database is root-owned; try sudo"
        } else {
            ""
        };
        bail!("sqlite3 exited with {}{}", status, hint);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// crashes

fn cmd_crashes(count: usize, show: Option<String>) -> Result<()> {
    let mut reports: Vec<(SystemTime, std::path::PathBuf)> = Vec::new();
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

    if reports.is_empty() {
        println!("no agent crash reports found in:");
        for dir in paths::crash_report_dirs() {
            println!("  {}", dir.display());
        }
        if unsafe { libc::geteuid() } != 0 {
            println!("(daemon crash reports in /Library/Logs/DiagnosticReports may need sudo)");
        }
        return Ok(());
    }

    if let Some(which) = show {
        let idx = if which == "latest" {
            0
        } else {
            which
                .parse::<usize>()
                .map_err(|_| anyhow!("--show takes an index from the list or 'latest'"))?
        };
        let (_, path) = reports
            .get(idx)
            .ok_or_else(|| anyhow!("no report at index {} (0-{})", idx, reports.len() - 1))?;
        println!("=== {} ===", path.display());
        print!("{}", read_with_sudo_hint(path)?);
        return Ok(());
    }

    println!("{:<4} {:<22} REPORT", "IDX", "MODIFIED");
    for (i, (mtime, path)) in reports.iter().take(count).enumerate() {
        let when: chrono::DateTime<chrono::Local> = (*mtime).into();
        println!(
            "{:<4} {:<22} {}",
            i,
            when.format("%Y-%m-%d %H:%M:%S"),
            path.display()
        );
    }
    println!("\nprint one with: lsman crashes --show <idx|latest>");
    Ok(())
}

// ---------------------------------------------------------------------------
// net

async fn cmd_net() -> Result<()> {
    let mut mgr = DaemonManager::new()?;
    mgr.update_status().await;
    let Some(pid) = mgr.get_pid() else {
        bail!("daemon process '{}' not running", mgr.get_process_name());
    };

    let conns = network::query_connections(pid)?;
    if conns.is_empty() {
        if unsafe { libc::geteuid() } != 0 {
            println!(
                "no socket data for pid {} — the daemon runs as root, so lsof needs sudo",
                pid
            );
        } else {
            println!("daemon (pid {}) has no open internet sockets", pid);
        }
        return Ok(());
    }
    println!("{:<5} {:<28} {:<28} STATE", "PROTO", "LOCAL", "REMOTE");
    for c in &conns {
        println!(
            "{:<5} {:<28} {:<28} {}",
            c.protocol,
            c.local,
            c.remote.as_deref().unwrap_or("-"),
            c.state.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// start / stop / restart

enum DaemonAction {
    Start,
    Stop,
    Restart,
}

async fn cmd_daemon_control(action: DaemonAction) -> Result<()> {
    let mut mgr = DaemonManager::new()?;
    mgr.update_status().await;

    match action {
        DaemonAction::Start => mgr.start_daemon()?,
        DaemonAction::Stop => mgr.stop_daemon()?,
        DaemonAction::Restart => mgr.restart_daemon()?,
    }

    tokio::time::sleep(Duration::from_millis(1500)).await;
    mgr.update_status().await;
    match mgr.get_pid() {
        Some(pid) => println!("daemon is {} (pid {})", mgr.get_state(), pid),
        None => println!("daemon is {}", mgr.get_state()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers

fn read_with_sudo_hint(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| map_perm_error(e, path))
}

fn map_perm_error(e: std::io::Error, path: &Path) -> anyhow::Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied && unsafe { libc::geteuid() } != 0 {
        anyhow!("permission denied reading {} — try sudo", path.display())
    } else {
        anyhow::Error::new(e).context(format!("reading {}", path.display()))
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_kb(kb: u64) -> String {
    format_bytes(kb * 1024)
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_level_from_debug_section() {
        let cfg = "[Agent]\nName=x\n\n[Debug]\nLogLevel2=8\nTraceXML=1\n";
        assert_eq!(read_log_level(cfg), Some(8));
    }

    #[test]
    fn read_level_ignores_other_sections() {
        let cfg = "[Agent]\nLogLevel2=5\n";
        assert_eq!(read_log_level(cfg), None);
    }

    #[test]
    fn read_level_case_insensitive_with_spaces() {
        let cfg = "[debug]\n  loglevel2 = 4\n";
        assert_eq!(read_log_level(cfg), Some(4));
    }

    #[test]
    fn set_level_replaces_existing() {
        let cfg = "[Debug]\nLogLevel2=3\nTraceXML=1\n";
        let out = set_log_level(cfg, 8);
        assert_eq!(read_log_level(&out), Some(8));
        assert!(out.contains("TraceXML=1"), "must preserve other keys");
        assert_eq!(out.matches("LogLevel2").count(), 1);
    }

    #[test]
    fn set_level_inserts_into_existing_section() {
        let cfg = "[Agent]\nName=x\n\n[Debug]\nTraceXML=1\n";
        let out = set_log_level(cfg, 5);
        assert_eq!(read_log_level(&out), Some(5));
        assert!(out.contains("[Agent]\nName=x"));
    }

    #[test]
    fn set_level_appends_section_when_missing() {
        let cfg = "[Agent]\nName=x\n";
        let out = set_log_level(cfg, 8);
        assert_eq!(read_log_level(&out), Some(8));
        assert!(out.contains("[Debug]"));
    }

    #[test]
    fn set_level_on_empty_config() {
        let out = set_log_level("", 8);
        assert_eq!(read_log_level(&out), Some(8));
    }

    #[test]
    fn parse_levels() {
        assert_eq!(parse_level("8").unwrap(), 8);
        assert_eq!(parse_level("trace5").unwrap(), 8);
        assert_eq!(parse_level("INFO").unwrap(), 3);
        assert!(parse_level("9").is_err());
        assert!(parse_level("bogus").is_err());
    }

    #[test]
    fn error_line_detection() {
        assert!(is_error_line("07-04 12:00:00 webSocNix.cpp(120) -E WebSock connect failed"));
        assert!(is_error_line("something Exception thrown"));
        assert!(!is_error_line("07-04 12:00:00 threadColl.cpp(88) -I Coll snapshot ok"));
    }
}
