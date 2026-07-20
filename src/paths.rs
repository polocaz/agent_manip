//! Platform facts about the LSI (SysTrack) agent installation.
//!
//! Paths verified against the agent source (Agent/lsiagentd — lsiFiles.cpp,
//! osx_installer/). Everything else in lsman should get agent paths from here.

use std::path::PathBuf;

/// launchd label for the macOS daemon (LaunchDaemon, system domain).
pub const LAUNCHD_LABEL: &str = "com.lakesidesoftware.lsiagentd";
/// Installed launchd plist on macOS.
pub const LAUNCHD_PLIST: &str = "/Library/LaunchDaemons/com.lakesidesoftware.lsiagentd.plist";
/// systemd service name on Linux.
pub const SYSTEMD_SERVICE: &str = "lsiagent";
/// Process name to look for.
pub const DAEMON_PROCESS_NAME: &str = "lsiagentd";

/// Agent runtime base directory (`<base>` in the agent docs).
pub fn base_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/Lakeside Software")
    } else {
        PathBuf::from("/var/opt/lsiagent")
    }
}

/// Agent config file (INI-style; `[Debug] LogLevel2=` controls trace level).
pub fn config_path() -> PathBuf {
    base_dir().join("lsiagent.cfg")
}

/// Agent SQLite database. `LSMAN_DB` overrides it so `lsman db`/the dashboard
/// can inspect an offline snapshot (e.g. a support drop's collect_copy.sqlite3)
/// instead of the live install.
pub fn database_path() -> PathBuf {
    if let Ok(p) = std::env::var("LSMAN_DB") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    base_dir().join("database").join("collect.sqlite3")
}

/// Master-delivered agent profile blob, written next to the config
/// (verified on a live macOS agent install; root-only permissions).
pub fn profile_path() -> PathBuf {
    base_dir().join("profile")
}

/// Daemon binary location.
pub fn daemon_binary() -> PathBuf {
    if cfg!(target_os = "macos") {
        base_dir().join("lsiagentd")
    } else {
        PathBuf::from("/opt/lsiagent/bin/lsiagentd")
    }
}

/// A log file the agent is known to write.
#[derive(Debug, Clone)]
pub struct KnownLog {
    /// Short name used to refer to the log on the command line.
    pub name: String,
    pub path: PathBuf,
    pub description: &'static str,
}

/// All log files the agent writes, in display order. Includes logs that don't
/// exist yet (callers check `path.exists()`).
pub fn known_logs() -> Vec<KnownLog> {
    let base = base_dir();
    let mut logs = Vec::new();

    if cfg!(target_os = "macos") {
        logs.push(KnownLog {
            name: "agent".into(),
            path: base.join("lsiagent.log"),
            description: "main daemon log (rotated by newsyslog)",
        });
    } else {
        // Linux self-rotates across lsiagent1..5.log at 5 MB (threadLog.cpp)
        for i in 1..=5 {
            logs.push(KnownLog {
                name: format!("agent{}", i),
                path: base.join(format!("lsiagent{}.log", i)),
                description: "main daemon log (5 MB self-rotation)",
            });
        }
    }

    logs.push(KnownLog {
        name: "webcom".into(),
        path: base.join("lsiwebcom.log"),
        description: "LsiWebCom / uplink to master",
    });
    logs.push(KnownLog {
        name: "sensors".into(),
        path: base.join("LSSensorEngine.log"),
        description: "sensor engine",
    });
    logs.push(KnownLog {
        name: "slow-sql".into(),
        path: base.join("slow_queries.log"),
        description: "slow SQL queries",
    });
    logs.push(KnownLog {
        name: "inventory".into(),
        path: base.join("Inventory.log"),
        description: "inventory collection",
    });

    if cfg!(target_os = "macos") {
        logs.push(KnownLog {
            name: "statusbar".into(),
            path: base.join("lsistatusbar.log"),
            description: "LsiStatusBar app + XPC bridge",
        });
        // Per-user LsiUser helper logs: Users/SysTrackManagementUser_<UID>.log
        if let Ok(entries) = std::fs::read_dir(base.join("Users")) {
            let mut user_logs: Vec<KnownLog> = entries
                .flatten()
                .filter_map(|e| {
                    let fname = e.file_name().to_string_lossy().into_owned();
                    let uid = fname
                        .strip_prefix("SysTrackManagementUser_")?
                        .strip_suffix(".log")?
                        .to_string();
                    Some(KnownLog {
                        name: format!("user-{}", uid),
                        path: e.path(),
                        description: "per-user LsiUser helper",
                    })
                })
                .collect();
            user_logs.sort_by(|a, b| a.name.cmp(&b.name));
            logs.extend(user_logs);
        }
    }

    logs
}

/// Look up a known log by its short name.
pub fn find_log(name: &str) -> Option<KnownLog> {
    known_logs().into_iter().find(|l| l.name == name)
}

/// Directories that may contain agent crash reports, most relevant first.
pub fn crash_report_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        // Daemon (runs as root) crashes land in the system dir; LsiStatusBar /
        // SysTrackManagementUser in the per-user dir.
        dirs.push(PathBuf::from("/Library/Logs/DiagnosticReports"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Logs/DiagnosticReports"));
        }
    }
    dirs
}

/// File-name prefixes of agent-related crash reports.
pub const CRASH_REPORT_PREFIXES: &[&str] =
    &["lsiagentd", "LsiStatusBar", "SysTrackManagementUser"];
