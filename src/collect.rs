//! `lsman collect` — bundle everything a support case needs into one
//! attachable .tar.gz: diagnostic report, config, size-capped log copies,
//! crash reports, socket/service state. Fills the "no mac/linux CLI diag
//! collector" gap from the agent bug-investigation runbook.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;

use crate::daemon::DaemonManager;
use crate::network;
use crate::paths;
use crate::report;

pub struct CollectOptions {
    /// Directory the bundle is written into.
    pub output_dir: PathBuf,
    /// Cap per copied log file, in MB (the tail is kept).
    pub max_log_mb: u64,
    /// Error-triage window for the report, e.g. "24h"; None = whole logs.
    pub since: Option<String>,
}

pub async fn run(opts: CollectOptions) -> Result<PathBuf> {
    let since = match &opts.since {
        Some(s) => Some(
            crate::triage::parse_since(s)
                .ok_or_else(|| anyhow!("bad --since '{}' (use e.g. 90s, 30m, 24h, 7d)", s))?,
        ),
        None => None,
    };

    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "host".to_string());
    let stamp = Utc::now().format("%Y%m%d-%H%M%SZ");
    let bundle_name = format!("lsman-diag-{}-{}", hostname, stamp);
    let staging = opts.output_dir.join(&bundle_name);
    fs::create_dir_all(&staging)
        .with_context(|| format!("creating staging dir {}", staging.display()))?;

    let mut manifest: Vec<String> = Vec::new();

    // Diagnostic report (includes error triage — the slow part)
    eprintln!("gathering diagnostic report…");
    let mut mgr = DaemonManager::new()?;
    let mut snap = report::gather(&mut mgr).await;
    snap.errors = Some(report::scan_errors(since, opts.since.clone()));
    fs::write(staging.join("report.md"), report::to_markdown(&snap))?;
    fs::write(
        staging.join("report.json"),
        serde_json::to_string_pretty(&snap)?,
    )?;
    manifest.push("report.md / report.json — lsman diagnostic report".to_string());

    // Config
    let cfg = paths::config_path();
    match copy_capped(&cfg, &staging.join("lsiagent.cfg"), u64::MAX) {
        Ok(_) => manifest.push("lsiagent.cfg — agent config".to_string()),
        Err(e) => manifest.push(format!("lsiagent.cfg — NOT captured: {}", e)),
    }

    // Logs, tail-capped so a 500 MB agent log doesn't become a 500 MB ticket
    // attachment
    let cap = opts.max_log_mb.saturating_mul(1024 * 1024);
    let logs_dir = staging.join("logs");
    fs::create_dir_all(&logs_dir)?;
    for log in paths::known_logs() {
        if !log.path.exists() {
            continue;
        }
        let fname = log
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.log", log.name));
        match copy_capped(&log.path, &logs_dir.join(&fname), cap) {
            Ok(truncated) => manifest.push(format!(
                "logs/{} — {}{}",
                fname,
                log.description,
                if truncated {
                    format!(" (TRUNCATED to last {} MB)", opts.max_log_mb)
                } else {
                    String::new()
                }
            )),
            Err(e) => manifest.push(format!("logs/{} — NOT captured: {}", fname, e)),
        }
    }

    // Crash reports (small text files; take the 10 newest)
    let crashes = report::list_crash_reports();
    if !crashes.is_empty() {
        let crash_dir = staging.join("crashes");
        fs::create_dir_all(&crash_dir)?;
        for (_, path) in crashes.iter().take(10) {
            let fname = path.file_name().map(|f| f.to_string_lossy().into_owned());
            let Some(fname) = fname else { continue };
            match copy_capped(path, &crash_dir.join(&fname), u64::MAX) {
                Ok(_) => manifest.push(format!("crashes/{}", fname)),
                Err(e) => manifest.push(format!("crashes/{} — NOT captured: {}", fname, e)),
            }
        }
    }

    // Live socket + service-manager state
    if let Some(pid) = snap.daemon.pid {
        if let Ok(conns) = network::query_connections(pid) {
            let mut out = String::from("PROTO LOCAL REMOTE STATE\n");
            for c in &conns {
                out.push_str(&format!(
                    "{} {} {} {}\n",
                    c.protocol,
                    c.local,
                    c.remote.as_deref().unwrap_or("-"),
                    c.state.as_deref().unwrap_or("-")
                ));
            }
            fs::write(staging.join("net.txt"), out)?;
            manifest.push("net.txt — daemon sockets (lsof)".to_string());
        }
    }
    if let Ok(status) = mgr.get_service_status() {
        fs::write(staging.join("service.txt"), status)?;
        manifest.push("service.txt — launchctl/systemctl status".to_string());
    }

    let mut manifest_body = format!(
        "lsman {} diagnostic bundle — {}\ngenerated {}\nlog timestamps are UTC\n\n",
        env!("CARGO_PKG_VERSION"),
        hostname,
        snap.generated_utc
    );
    for line in &manifest {
        manifest_body.push_str(line);
        manifest_body.push('\n');
    }
    fs::write(staging.join("MANIFEST.txt"), manifest_body)?;

    // Pack and clean up
    let tarball = opts.output_dir.join(format!("{}.tar.gz", bundle_name));
    let status = Command::new("tar")
        .arg("czf")
        .arg(&tarball)
        .arg("-C")
        .arg(&opts.output_dir)
        .arg(&bundle_name)
        .status()
        .map_err(|e| anyhow!("failed to run tar: {}", e))?;
    if !status.success() {
        bail!("tar exited with {} (staging left at {})", status, staging.display());
    }
    fs::remove_dir_all(&staging).ok();

    Ok(tarball)
}

/// Copy `src` to `dst`, keeping only the last `cap` bytes for oversized files.
/// Returns whether the copy was truncated.
fn copy_capped(src: &Path, dst: &Path, cap: u64) -> Result<bool> {
    let mut file = fs::File::open(src).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied && unsafe { libc::geteuid() } != 0 {
            anyhow!("permission denied (try sudo)")
        } else {
            anyhow!(e)
        }
    })?;
    let len = file.metadata()?.len();
    let truncated = len > cap;
    if truncated {
        file.seek(SeekFrom::Start(len - cap))?;
    }

    let mut out = fs::File::create(dst)?;
    if truncated {
        writeln!(
            out,
            "=== lsman collect: file truncated, last {} of {} bytes ===",
            cap, len
        )?;
    }
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
    }
    Ok(truncated)
}
