//! Non-interactive subcommands for debugging the agent from the command line.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, Result};
use clap::Subcommand;

use crate::collect;
use crate::daemon::{DaemonManager, DaemonState};
use crate::network;
use crate::paths;
use crate::report;
use crate::serve;
use crate::triage::{self, is_error_line};

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
    /// Triage scan: group error/crash lines by the source site that logged them
    Errors {
        /// Restrict to one log name (default: all main agent logs)
        #[arg(long)]
        log: Option<String>,
        /// Grouped mode: max groups to show; raw mode: max lines per log
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Only lines newer than this window, e.g. 90s, 30m, 24h, 7d
        #[arg(long)]
        since: Option<String>,
        /// Print raw matching lines instead of the grouped summary
        #[arg(long)]
        raw: bool,
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
        /// Show a table's newest rows with string-IDs resolved via SASTR/SASTRUSER
        #[arg(long, conflicts_with_all = ["sql", "find"])]
        table: Option<String>,
        /// Max rows for --table
        #[arg(long, default_value_t = 25)]
        limit: usize,
        /// Search SASTR/SASTRUSER for a substring ("has this box ever seen X?")
        #[arg(long, conflicts_with = "sql")]
        find: Option<String>,
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
    /// One-shot markdown diagnostic report (paste into a ticket)
    Report {
        /// Error-triage window, e.g. 24h (default: whole logs)
        #[arg(long)]
        since: Option<String>,
        /// Write to a file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Bundle report + config + capped logs + crash reports into a .tar.gz
    Collect {
        /// Directory to write the bundle into
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Keep at most this many MB (the tail) of each copied log
        #[arg(long, default_value_t = 50)]
        max_log_mb: u64,
        /// Error-triage window for the included report, e.g. 24h
        #[arg(long)]
        since: Option<String>,
    },
    /// Serve a web dashboard (status, errors, logs, daemon control, downloads, cfg editor)
    Serve {
        #[arg(short, long, default_value_t = 7171)]
        port: u16,
        /// Bind address; the dashboard is unauthenticated, keep it loopback
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },
    /// Resolve a log line's <file>(<line>) to the agent source checkout
    Where {
        /// "webSocNix.cpp(120)", "webSocNix.cpp:120", or a pasted log line
        reference: String,
    },
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
        CliCommand::Errors { log, lines, since, raw } => cmd_errors(log, lines, since, raw),
        CliCommand::Config { path } => cmd_config(path),
        CliCommand::Trace { level, restart } => cmd_trace(level, restart).await,
        CliCommand::Db { sql, table, limit, find } => cmd_db(sql, table, limit, find),
        CliCommand::Crashes { count, show } => cmd_crashes(count, show),
        CliCommand::Net => cmd_net().await,
        CliCommand::Report { since, output } => cmd_report(since, output).await,
        CliCommand::Collect { output, max_log_mb, since } => {
            let tarball = collect::run(collect::CollectOptions {
                output_dir: output,
                max_log_mb,
                since,
            })
            .await?;
            println!("diagnostic bundle: {}", tarball.display());
            Ok(())
        }
        CliCommand::Serve { port, bind } => serve::run(serve::ServeOptions { bind, port }).await,
        CliCommand::Where { reference } => cmd_where(&reference),
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

fn errors_target_logs(log_name: Option<String>) -> Result<Vec<paths::KnownLog>> {
    match log_name {
        Some(name) => Ok(vec![paths::find_log(&name)
            .ok_or_else(|| anyhow!("unknown log '{}' — run `lsman logs` to list names", name))?]),
        // Default to the main daemon log(s): agent / agent1..5
        None => Ok(paths::known_logs()
            .into_iter()
            .filter(|l| l.name.starts_with("agent"))
            .collect()),
    }
}

fn parse_since_arg(since: &Option<String>) -> Result<Option<chrono::Duration>> {
    match since {
        Some(s) => Ok(Some(triage::parse_since(s).ok_or_else(|| {
            anyhow!("bad --since '{}' (use e.g. 90s, 30m, 24h, 7d)", s)
        })?)),
        None => Ok(None),
    }
}

fn cmd_errors(
    log_name: Option<String>,
    max_lines: usize,
    since: Option<String>,
    raw: bool,
) -> Result<()> {
    let window = parse_since_arg(&since)?;
    let logs = errors_target_logs(log_name)?;

    if raw {
        return cmd_errors_raw(logs, max_lines, window);
    }

    let scan = report::scan_errors_in(&logs, window, since.clone());
    let label = since.map(|s| format!(" in the last {}", s)).unwrap_or_default();
    if scan.groups.is_empty() {
        println!("no error/crash lines found{}", label);
    } else {
        println!(
            "{} error/crash lines across {} source sites{} (timestamps are UTC)\n",
            scan.total_matches,
            scan.groups.len(),
            label
        );
        println!(
            "{:<10} {:<34} {:>6}  {:<15} {:<15}",
            "LOG", "SITE", "COUNT", "FIRST SEEN", "LAST SEEN"
        );
        for g in scan.groups.iter().take(max_lines) {
            println!(
                "{:<10} {:<34} {:>6}  {:<15} {:<15}",
                g.log,
                g.site,
                g.count,
                g.first_seen.as_deref().unwrap_or("-"),
                g.last_seen.as_deref().unwrap_or("-")
            );
        }
        if scan.groups.len() > max_lines {
            println!("… and {} more sites (raise -n)", scan.groups.len() - max_lines);
        }
        println!("\nmost recent line per site:");
        for g in scan.groups.iter().take(max_lines) {
            println!("  {}", g.sample);
        }
        println!(
            "\nresolve a site to source with: lsman where \"<site>\"  |  raw lines: lsman errors --raw"
        );
    }
    if !scan.unreadable_logs.is_empty() {
        println!(
            "unreadable logs (permissions — try sudo): {}",
            scan.unreadable_logs.join(", ")
        );
    }
    Ok(())
}

fn cmd_errors_raw(
    logs: Vec<paths::KnownLog>,
    max_lines: usize,
    window: Option<chrono::Duration>,
) -> Result<()> {
    let now = chrono::Utc::now();
    let cutoff = window.map(|d| now.naive_utc() - d);

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
            if !is_error_line(&line) {
                continue;
            }
            if let Some(cutoff) = cutoff {
                match triage::parse_line(&line, now).timestamp {
                    Some(ts) if ts >= cutoff => {}
                    _ => continue,
                }
            }
            match_count += 1;
            if matches.len() == max_lines {
                matches.pop_front();
            }
            matches.push_back((line_no + 1, line));
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
// report

async fn cmd_report(since: Option<String>, output: Option<PathBuf>) -> Result<()> {
    let window = parse_since_arg(&since)?;
    let mut mgr = DaemonManager::new()?;
    eprintln!("gathering diagnostic report…");
    let mut snap = report::gather(&mut mgr).await;
    snap.errors = Some(report::scan_errors(window, since));
    let md = report::to_markdown(&snap);
    match output {
        Some(path) => {
            fs::write(&path, md)?;
            println!("report written to {}", path.display());
        }
        None => print!("{}", md),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// where — log site -> agent source checkout

/// Agent source roots to try, most specific first.
fn agent_src_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(env_root) = std::env::var("LSMAN_AGENT_SRC") {
        roots.push(PathBuf::from(env_root));
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(&home).join("src/systrack/Agent"));
        roots.push(PathBuf::from(&home).join("src/systrack-col1710/Agent"));
    }
    roots
}

/// Pull a `<file>` + `<line>` out of "f.cpp(120)", "f.cpp:120", a bare file
/// name, or a whole pasted log line.
fn parse_source_ref(reference: &str) -> Option<(String, u32)> {
    let reference = reference.trim();
    // A pasted log line: find the first token shaped like <file>(<line>)
    if reference.contains(char::is_whitespace) {
        return reference.split_whitespace().find_map(|tok| {
            triage::split_site(tok).map(|(f, l)| (f.to_string(), l))
        });
    }
    if let Some((file, line)) = triage::split_site(reference) {
        return Some((file.to_string(), line));
    }
    if let Some((file, line)) = reference.rsplit_once(':') {
        if let Ok(line) = line.parse() {
            return Some((file.to_string(), line));
        }
    }
    Some((reference.to_string(), 0))
}

/// The agent logs the source file without its extension (`dbConnNix(1119)`),
/// so an extensionless query matches any source file with that stem.
fn source_file_matches(fname: &str, query: &str) -> bool {
    if fname.eq_ignore_ascii_case(query) {
        return true;
    }
    if query.contains('.') {
        return false;
    }
    match fname.rsplit_once('.') {
        Some((stem, ext)) => {
            matches!(ext.to_ascii_lowercase().as_str(), "cpp" | "c" | "cc" | "h" | "hpp" | "m" | "mm")
                && stem.eq_ignore_ascii_case(query)
        }
        None => false,
    }
}

fn find_file_named(dir: &Path, name: &str, hits: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().into_owned();
        if fname.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            find_file_named(&path, name, hits);
        } else if source_file_matches(&fname, name) {
            hits.push(path);
        }
    }
}

fn cmd_where(reference: &str) -> Result<()> {
    let (file, line) = parse_source_ref(reference)
        .ok_or_else(|| anyhow!("no <file>(<line>) reference found in '{}'", reference))?;

    let root = agent_src_roots()
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or_else(|| {
            anyhow!(
                "no agent source checkout found — set LSMAN_AGENT_SRC to your Agent/ directory"
            )
        })?;

    let mut hits = Vec::new();
    find_file_named(&root, &file, &mut hits);
    if hits.is_empty() {
        bail!("'{}' not found under {}", file, root.display());
    }
    // Implementation files first: they're what log sites almost always point at
    hits.sort_by_key(|p| {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase());
        match ext.as_deref() {
            Some("cpp" | "cc" | "c" | "m" | "mm") => 0,
            _ => 1,
        }
    });

    for hit in &hits {
        if line > 0 {
            println!("{}:{}", hit.display(), line);
        } else {
            println!("{}", hit.display());
        }
    }

    // Show the emitting code from the first hit that actually has that line
    if line > 0 {
        let mut shown = false;
        for hit in &hits {
            let Ok(content) = fs::read_to_string(hit) else { continue };
            let all: Vec<&str> = content.lines().collect();
            let idx = (line as usize) - 1;
            if idx >= all.len() {
                continue;
            }
            println!();
            let from = idx.saturating_sub(3);
            let to = (idx + 4).min(all.len());
            for (i, text) in all.iter().enumerate().take(to).skip(from) {
                let marker = if i == idx { ">" } else { " " };
                println!("{} {:>5} | {}", marker, i + 1, text);
            }
            shown = true;
            break;
        }
        if !shown {
            println!(
                "\n(no matched file reaches line {} — the customer's agent build likely differs \
                 from this checkout)",
                line
            );
        }
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

pub(crate) fn level_name(level: i64) -> &'static str {
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
pub(crate) fn read_log_level(cfg: &str) -> Option<i64> {
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
pub(crate) fn set_log_level(cfg: &str, level: i64) -> String {
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

fn cmd_db(
    sql: Option<String>,
    table: Option<String>,
    limit: usize,
    find: Option<String>,
) -> Result<()> {
    let db = paths::database_path();
    if !db.exists() {
        bail!("database not found at {}", db.display());
    }

    if let Some(pattern) = find {
        return cmd_db_find(&pattern);
    }
    if let Some(name) = table {
        return cmd_db_table(&name, limit);
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

/// `lsman db --find X`: substring search over the SASTR/SASTRUSER string
/// tables. Inventory tables reference strings by ID, so this is the fastest
/// "has this endpoint ever seen X in any form" check — zero matches means no
/// table can reference it.
fn cmd_db_find(pattern: &str) -> Result<()> {
    const CAP: usize = 200;
    let rows = crate::db::search_strings(pattern, CAP)?;
    if rows.is_empty() {
        println!(
            "no SASTR/SASTRUSER string matches '{}' — conclusive: nothing on this endpoint references it in any form",
            pattern
        );
        return Ok(());
    }
    println!("{:<9} {:>10}  value", "scope", "string-id");
    for r in &rows {
        println!(
            "{:<9} {:>10}  {}",
            r.get("scope").and_then(|v| v.as_str()).unwrap_or("?"),
            r.get("STRINGID").and_then(|v| v.as_i64()).unwrap_or(-1),
            r.get("STRVALUE").and_then(|v| v.as_str()).unwrap_or("")
        );
    }
    if rows.len() >= CAP {
        println!("(capped at {} matches — narrow the pattern)", CAP);
    }
    Ok(())
}

/// `lsman db --table NAME`: newest rows with integer string-IDs resolved to
/// their SASTR/SASTRUSER values, so inventory tables are readable without
/// hand-writing joins.
fn cmd_db_table(name: &str, limit: usize) -> Result<()> {
    let rows = crate::db::table_rows(name, limit)?;
    if rows.is_empty() {
        println!("{}: empty table", name);
        return Ok(());
    }
    let resolved = crate::db::resolve_string_ids(&rows);

    // Column set from the first row (serde_json objects sort keys, so the
    // order is alphabetical, not schema order).
    let cols: Vec<String> = rows[0]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let mut grid: Vec<Vec<String>> = Vec::with_capacity(rows.len() + 1);
    grid.push(
        cols.iter()
            .map(|c| {
                if resolved.contains_key(c) {
                    format!("{}*", c)
                } else {
                    c.clone()
                }
            })
            .collect(),
    );
    for row in &rows {
        grid.push(
            cols.iter()
                .map(|c| {
                    render_db_cell(row.as_object().and_then(|o| o.get(c)), c, &resolved)
                })
                .collect(),
        );
    }

    let mut widths = vec![0usize; cols.len()];
    for row in &grid {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    for row in &grid {
        let line = row
            .iter()
            .enumerate()
            .map(|(i, cell)| format!("{:<width$}", cell, width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{}", line.trim_end());
    }
    println!("\n{} rows (newest first, max {})", rows.len(), limit);
    if !resolved.is_empty() {
        println!("* string-IDs resolved via SASTR/SASTRUSER; unresolved IDs shown raw");
    }
    Ok(())
}

fn render_db_cell(
    value: Option<&serde_json::Value>,
    col: &str,
    resolved: &crate::db::ResolvedStrings,
) -> String {
    let Some(value) = value else { return String::new() };
    if let (Some(map), Some(id)) = (resolved.get(col), value.as_i64()) {
        if let Some(s) = map.get(&id) {
            return clip_cell(s);
        }
    }
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => clip_cell(s),
        other => other.to_string(),
    }
}

/// Keep table cells terminal-friendly; resolved paths/cmdlines can be long.
fn clip_cell(s: &str) -> String {
    const MAX: usize = 48;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let mut t: String = s.chars().take(MAX - 1).collect();
    t.push('…');
    t
}

// ---------------------------------------------------------------------------
// crashes

fn cmd_crashes(count: usize, show: Option<String>) -> Result<()> {
    let reports = report::list_crash_reports();

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

pub(crate) fn format_bytes(bytes: u64) -> String {
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

pub(crate) fn format_kb(kb: u64) -> String {
    format_bytes(kb * 1024)
}

pub(crate) fn format_duration(secs: u64) -> String {
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
    fn source_ref_parsing() {
        assert_eq!(
            parse_source_ref("webSocNix.cpp(120)"),
            Some(("webSocNix.cpp".to_string(), 120))
        );
        assert_eq!(
            parse_source_ref("webSocNix.cpp:120"),
            Some(("webSocNix.cpp".to_string(), 120))
        );
        assert_eq!(
            parse_source_ref("07-04 09:15:42 webSocNix.cpp(120) -E connect failed"),
            Some(("webSocNix.cpp".to_string(), 120))
        );
        assert_eq!(
            parse_source_ref("dbConnNix.cpp"),
            Some(("dbConnNix.cpp".to_string(), 0))
        );
        // real agent lines log the site without extension, padded into a column
        assert_eq!(
            parse_source_ref("07-02 12:13:32 DbUtils(5009)            -E        Failed to create"),
            Some(("DbUtils".to_string(), 5009))
        );
    }

    #[test]
    fn source_file_stem_matching() {
        assert!(source_file_matches("dbConnNix.cpp", "dbConnNix"));
        assert!(source_file_matches("dbConnNix.h", "dbconnnix"));
        assert!(source_file_matches("webSocNix.cpp", "webSocNix.cpp"));
        assert!(!source_file_matches("dbConnNix.o", "dbConnNix"));
        assert!(!source_file_matches("dbConnNixTest.cpp", "dbConnNix"));
    }
}
