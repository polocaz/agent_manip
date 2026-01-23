use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::sync::Mutex;
use sysinfo::System;
use anyhow::{Result, anyhow};

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

#[derive(Debug)]
pub struct ProcessStats {
    pub pid: Option<u32>,
    pub cpu_usage: f32,
    pub memory_usage: u64, // in KB
    pub virtual_memory: u64, // in KB
    pub thread_count: usize,
    pub start_time: u64,
    pub uptime_seconds: u64,
    pub open_files: usize, // Number of open file descriptors
    // Additional metrics
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub context_switches: u64,
    pub page_faults: u64,
    pub system_load_avg: f32,
    pub system_memory_total: u64, // in KB
    pub system_memory_used: u64, // in KB
    pub ppid: Option<u32>,
    pub priority: i32,
    pub state: String,
}

impl Default for ProcessStats {
    fn default() -> Self {
        Self {
            pid: None,
            cpu_usage: 0.0,
            memory_usage: 0,
            virtual_memory: 0,
            thread_count: 0,
            start_time: 0,
            uptime_seconds: 0,
            open_files: 0,
            // Additional metrics
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            context_switches: 0,
            page_faults: 0,
            system_load_avg: 0.0,
            system_memory_total: 0,
            system_memory_used: 0,
            ppid: None,
            priority: 0,
            state: "Unknown".to_string(),
        }
    }
}

pub struct DaemonManager {
    system: Arc<Mutex<System>>,
    daemon_name: String,
    daemon_path: String,
    service_name: String,
    current_pid: Option<u32>,
    state: DaemonState,
    process_stats: ProcessStats,
    last_start_attempt: std::time::Instant,
    use_systemctl: bool,
}

impl DaemonManager {
    pub fn new() -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        // Check if systemctl is available (Linux with systemd)
        let use_systemctl = cfg!(target_os = "linux") && 
            Command::new("systemctl")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

        // Determine daemon name and path based on platform
        let (daemon_name, daemon_path) = if cfg!(target_os = "windows") {
            ("LsiAgent.exe".to_string(), "C:\\Program Files\\Lakeside Software\\LsiAgent.exe".to_string())
        } else if cfg!(target_os = "macos") {
            ("lsiagentd".to_string(), "/Library/Application Support/Lakeside Software/lsiagentd".to_string())
        } else {
            // Linux and other Unix-like systems
            ("lsiagentd".to_string(), "/opt/lsiagent/bin/lsiagentd".to_string())
        };

        Ok(Self {
            system: Arc::new(Mutex::new(system)),
            daemon_name,
            daemon_path,
            service_name: "lsiagent".to_string(), // systemctl service name
            current_pid: None,
            state: DaemonState::Unknown,
            process_stats: ProcessStats::default(),
            last_start_attempt: std::time::Instant::now(),
            use_systemctl,
        })
    }

    pub async fn update_status(&mut self) {
        let mut system = self.system.lock().await;
        system.refresh_all();

        // Try to find the daemon process
        self.current_pid = None;
        for (pid, process) in system.processes() {
            let process_name = process.name().to_lowercase();
            let daemon_name_lower = self.daemon_name.to_lowercase();
            
            // Check for exact match or common variations
            if process_name == daemon_name_lower ||
               process_name.contains(&daemon_name_lower.trim_end_matches(".exe")) ||
               process_name.contains("lsiagent") {
                self.current_pid = Some(pid.as_u32());
                break;
            }
        }

        // Update state based on PID
        self.state = match self.current_pid {
            Some(pid) => {
                if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
                    let disk_usage = process.disk_usage();
                    self.process_stats = ProcessStats {
                        pid: Some(pid),
                        cpu_usage: process.cpu_usage(),
                        memory_usage: process.memory() / 1024, // Convert to KB
                        virtual_memory: process.virtual_memory() / 1024, // Convert to KB
                        thread_count: 0, // TODO: thread_count not directly available in sysinfo
                        start_time: process.start_time(),
                        uptime_seconds: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            .saturating_sub(process.start_time()),
                        open_files: get_open_file_count(pid),
                        // Additional metrics
                        disk_read_bytes: disk_usage.total_read_bytes,
                        disk_write_bytes: disk_usage.total_written_bytes,
                        network_rx_bytes: 0, // TODO: Network stats per process not easily available
                        network_tx_bytes: 0, // TODO: Network stats per process not easily available
                        context_switches: 0, // TODO: Context switches not available in sysinfo
                        page_faults: 0, // TODO: Page faults not available in sysinfo
                        system_load_avg: sysinfo::System::load_average().one as f32,
                        system_memory_total: system.total_memory() / 1024, // Convert to KB
                        system_memory_used: system.used_memory() / 1024, // Convert to KB
                        ppid: process.parent().map(|p| p.as_u32()),
                        priority: 0, // TODO: priority not available in sysinfo
                        state: format!("{:?}", process.status()),
                    };
                    DaemonState::Running
                } else {
                    DaemonState::Crashed
                }
            }
            None => DaemonState::Stopped,
        };
    }

    pub fn start_daemon(&mut self) -> Result<()> {
        if self.state == DaemonState::Running {
            return Err(anyhow!("Daemon is already running"));
        }

        self.state = DaemonState::Starting;
        self.last_start_attempt = std::time::Instant::now();

        if self.use_systemctl {
            // Use systemctl to start the service
            let output = Command::new("systemctl")
                .args(["start", &self.service_name])
                .output()
                .map_err(|e| anyhow!("Failed to run systemctl start: {}", e))?;

            if !output.status.success() {
                self.state = DaemonState::Stopped;
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("systemctl start failed: {}", stderr));
            }
        } else {
            // Fallback to direct process start
            let child = Command::new(&self.daemon_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| anyhow!("Failed to start daemon process: {}", e))?;

            self.current_pid = Some(child.id());
        }

        Ok(())
    }

    pub fn stop_daemon(&mut self) -> Result<()> {
        self.state = DaemonState::Stopping;

        if self.use_systemctl {
            // Use systemctl to stop the service
            let output = Command::new("systemctl")
                .args(["stop", &self.service_name])
                .output()
                .map_err(|e| anyhow!("Failed to run systemctl stop: {}", e))?;

            if !output.status.success() {
                self.state = DaemonState::Running; // Revert state if stop failed
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("systemctl stop failed: {}", stderr));
            }
        } else {
            // Fallback to direct process termination
            let pid = self.current_pid.ok_or_else(|| anyhow!("No daemon process running"))?;

            // Send SIGTERM first
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }

        Ok(())
    }

    pub fn get_service_status(&self) -> Result<String> {
        if !self.use_systemctl {
            return Ok("systemctl not available".to_string());
        }

        let output = Command::new("systemctl")
            .args(["status", &self.service_name, "--no-pager", "-l"])
            .output()
            .map_err(|e| anyhow!("Failed to run systemctl status: {}", e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(format!("Failed to get status: {}", stderr))
        }
    }

    pub fn get_state(&self) -> DaemonState {
        self.state
    }

    pub fn get_process_stats(&self) -> &ProcessStats {
        &self.process_stats
    }

    pub fn get_process_name(&self) -> &str {
        &self.daemon_name
    }

    pub fn is_using_systemctl(&self) -> bool {
        self.use_systemctl
    }
}

fn get_open_file_count(pid: u32) -> usize {
    // Try to count open file descriptors
    if cfg!(target_os = "linux") {
        // On Linux, count files in /proc/[pid]/fd/
        let fd_path = format!("/proc/{}/fd", pid);
        if let Ok(entries) = std::fs::read_dir(&fd_path) {
            return entries.count();
        }
    } else if cfg!(target_os = "macos") {
        // On macOS, try using lsof command
        if let Ok(output) = std::process::Command::new("lsof")
            .args(["-p", &pid.to_string()])
            .output()
        {
            if output.status.success() {
                // Count lines (excluding header)
                let output_str = String::from_utf8_lossy(&output.stdout);
                return output_str.lines().count().saturating_sub(1);
            }
        }
    }
    // Fallback: return 0 if we can't determine
    0
}