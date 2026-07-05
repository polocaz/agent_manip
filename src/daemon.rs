use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use sysinfo::System;

use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonState {
    Running,
    Stopped,
    Starting,
    Stopping,
    Crashed,
    Unknown,
}

impl std::fmt::Display for DaemonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonState::Running => write!(f, "Running"),
            DaemonState::Stopped => write!(f, "Stopped"),
            DaemonState::Starting => write!(f, "Starting"),
            DaemonState::Stopping => write!(f, "Stopping"),
            DaemonState::Crashed => write!(f, "Crashed"),
            DaemonState::Unknown => write!(f, "Unknown"),
        }
    }
}

/// How the daemon is managed on this host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    /// systemd (Linux)
    Systemctl,
    /// launchd (macOS) — the agent is a KeepAlive LaunchDaemon, so it MUST be
    /// stopped via launchctl; killing the process just gets it respawned.
    Launchctl,
    /// No service manager found; spawn/signal the process directly.
    Direct,
}

impl ServiceManager {
    pub fn name(&self) -> &'static str {
        match self {
            ServiceManager::Systemctl => "SYSTEMCTL",
            ServiceManager::Launchctl => "LAUNCHCTL",
            ServiceManager::Direct => "DIRECT",
        }
    }

    pub fn detect() -> Self {
        if cfg!(target_os = "macos") && Path::new(paths::LAUNCHD_PLIST).exists() {
            return ServiceManager::Launchctl;
        }
        if cfg!(target_os = "linux") {
            let has_systemctl = Command::new("systemctl")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if has_systemctl {
                return ServiceManager::Systemctl;
            }
        }
        ServiceManager::Direct
    }
}

#[derive(Debug, Default)]
pub struct ProcessStats {
    pub pid: Option<u32>,
    pub cpu_usage: f32,
    pub memory_usage: u64,   // in KB
    pub virtual_memory: u64, // in KB
    pub start_time: u64,
    pub uptime_seconds: u64,
    pub open_files: usize,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub system_load_avg: f32,
    pub system_memory_total: u64, // in KB
    pub system_memory_used: u64,  // in KB
    pub ppid: Option<u32>,
    pub state: String,
}

pub struct DaemonManager {
    system: System,
    manager: ServiceManager,
    current_pid: Option<u32>,
    state: DaemonState,
    process_stats: ProcessStats,
    service_status: String, // cached output of get_service_status()
}

impl DaemonManager {
    pub fn new() -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        Ok(Self {
            system,
            manager: ServiceManager::detect(),
            current_pid: None,
            state: DaemonState::Unknown,
            process_stats: ProcessStats::default(),
            service_status: String::new(),
        })
    }

    pub async fn update_status(&mut self) {
        self.system.refresh_all();

        // Find the daemon process by name
        self.current_pid = None;
        for (pid, process) in self.system.processes() {
            let process_name = process.name().to_lowercase();
            if process_name == paths::DAEMON_PROCESS_NAME
                || process_name.contains("lsiagent")
            {
                self.current_pid = Some(pid.as_u32());
                break;
            }
        }

        self.state = match self.current_pid {
            Some(pid) => {
                if let Some(process) = self.system.process(sysinfo::Pid::from_u32(pid)) {
                    let disk_usage = process.disk_usage();
                    self.process_stats = ProcessStats {
                        pid: Some(pid),
                        cpu_usage: process.cpu_usage(),
                        memory_usage: process.memory() / 1024,
                        virtual_memory: process.virtual_memory() / 1024,
                        start_time: process.start_time(),
                        // start_time 0 means the kernel refused the info (e.g.
                        // non-root looking at the root daemon on macOS)
                        uptime_seconds: if process.start_time() > 0 {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                                .saturating_sub(process.start_time())
                        } else {
                            0
                        },
                        open_files: get_open_file_count(pid),
                        disk_read_bytes: disk_usage.total_read_bytes,
                        disk_write_bytes: disk_usage.total_written_bytes,
                        system_load_avg: sysinfo::System::load_average().one as f32,
                        system_memory_total: self.system.total_memory() / 1024,
                        system_memory_used: self.system.used_memory() / 1024,
                        ppid: process.parent().map(|p| p.as_u32()),
                        state: format!("{:?}", process.status()),
                    };
                    DaemonState::Running
                } else {
                    DaemonState::Crashed
                }
            }
            None => {
                self.process_stats = ProcessStats::default();
                DaemonState::Stopped
            }
        };

        // Cache for the UI so drawing never has to shell out
        self.service_status = self
            .get_service_status()
            .unwrap_or_else(|e| e.to_string());
    }

    /// Cached service-manager status, refreshed by update_status().
    pub fn cached_service_status(&self) -> &str {
        &self.service_status
    }

    pub fn start_daemon(&mut self) -> Result<()> {
        if self.state == DaemonState::Running {
            return Err(anyhow!("Daemon is already running"));
        }
        self.state = DaemonState::Starting;

        let result = match self.manager {
            ServiceManager::Systemctl => {
                run_checked("systemctl", &["start", paths::SYSTEMD_SERVICE])
            }
            ServiceManager::Launchctl => {
                // If the job is loaded, kickstart it; otherwise bootstrap the plist.
                if launchctl_job_loaded() {
                    run_checked(
                        "launchctl",
                        &["kickstart", &format!("system/{}", paths::LAUNCHD_LABEL)],
                    )
                } else {
                    run_checked("launchctl", &["bootstrap", "system", paths::LAUNCHD_PLIST])
                }
            }
            ServiceManager::Direct => {
                let daemon_path = paths::daemon_binary();
                Command::new(&daemon_path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map(|child| {
                        self.current_pid = Some(child.id());
                    })
                    .map_err(|e| {
                        anyhow!("Failed to start {}: {}", daemon_path.display(), e)
                    })
            }
        };

        if result.is_err() {
            self.state = DaemonState::Stopped;
        }
        result
    }

    pub fn stop_daemon(&mut self) -> Result<()> {
        self.state = DaemonState::Stopping;

        let result = match self.manager {
            ServiceManager::Systemctl => {
                run_checked("systemctl", &["stop", paths::SYSTEMD_SERVICE])
            }
            ServiceManager::Launchctl => {
                // bootout unloads the job; a plain kill would be undone by KeepAlive.
                run_checked(
                    "launchctl",
                    &["bootout", &format!("system/{}", paths::LAUNCHD_LABEL)],
                )
            }
            ServiceManager::Direct => {
                let pid = self
                    .current_pid
                    .ok_or_else(|| anyhow!("No daemon process running"))?;
                // Graceful shutdown: the agent handles SIGTERM via StopAgent()
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
                Ok(())
            }
        };

        if result.is_err() {
            self.state = DaemonState::Running;
        }
        result
    }

    pub fn restart_daemon(&mut self) -> Result<()> {
        match self.manager {
            ServiceManager::Systemctl => {
                run_checked("systemctl", &["restart", paths::SYSTEMD_SERVICE])
            }
            ServiceManager::Launchctl => {
                if launchctl_job_loaded() {
                    run_checked(
                        "launchctl",
                        &["kickstart", "-k", &format!("system/{}", paths::LAUNCHD_LABEL)],
                    )
                } else {
                    run_checked("launchctl", &["bootstrap", "system", paths::LAUNCHD_PLIST])
                }
            }
            ServiceManager::Direct => {
                let _ = self.stop_daemon();
                std::thread::sleep(std::time::Duration::from_millis(500));
                self.start_daemon()
            }
        }
    }

    /// Human-readable service-manager status output (systemctl status /
    /// launchctl print), for display.
    pub fn get_service_status(&self) -> Result<String> {
        let output = match self.manager {
            ServiceManager::Systemctl => Command::new("systemctl")
                .args(["status", paths::SYSTEMD_SERVICE, "--no-pager", "-l"])
                .output(),
            ServiceManager::Launchctl => Command::new("launchctl")
                .args(["print", &format!("system/{}", paths::LAUNCHD_LABEL)])
                .output(),
            ServiceManager::Direct => {
                return Ok("no service manager - direct process management".to_string())
            }
        }
        .map_err(|e| anyhow!("Failed to query service status: {}", e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(format!("Failed to get status: {}", stderr.trim()))
        }
    }

    pub fn get_state(&self) -> DaemonState {
        self.state
    }

    pub fn get_pid(&self) -> Option<u32> {
        self.current_pid
    }

    pub fn get_process_stats(&self) -> &ProcessStats {
        &self.process_stats
    }

    pub fn get_process_name(&self) -> &str {
        paths::DAEMON_PROCESS_NAME
    }

    pub fn manager(&self) -> ServiceManager {
        self.manager
    }
}

fn launchctl_job_loaded() -> bool {
    Command::new("launchctl")
        .args(["print", &format!("system/{}", paths::LAUNCHD_LABEL)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_checked(cmd: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| anyhow!("Failed to run {} {}: {}", cmd, args.join(" "), e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let hint = if unsafe { libc::geteuid() } != 0 {
            " (not running as root — try sudo)"
        } else {
            ""
        };
        Err(anyhow!(
            "{} {} failed: {}{}",
            cmd,
            args.join(" "),
            stderr.trim(),
            hint
        ))
    }
}

fn get_open_file_count(pid: u32) -> usize {
    if cfg!(target_os = "linux") {
        let fd_path = format!("/proc/{}/fd", pid);
        if let Ok(entries) = std::fs::read_dir(&fd_path) {
            return entries.count();
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(output) = Command::new("lsof")
            .args(["-p", &pid.to_string()])
            .output()
        {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                return output_str.lines().count().saturating_sub(1);
            }
        }
    }
    0
}
