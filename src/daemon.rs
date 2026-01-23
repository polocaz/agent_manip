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
        }
    }
}

pub struct DaemonManager {
    system: Arc<Mutex<System>>,
    daemon_name: String,
    daemon_path: String,
    current_pid: Option<u32>,
    state: DaemonState,
    process_stats: ProcessStats,
    last_start_attempt: std::time::Instant,
}

impl DaemonManager {
    pub fn new() -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        Ok(Self {
            system: Arc::new(Mutex::new(system)),
            daemon_name: "telemetry-daemon".to_string(), // TODO: Make configurable
            daemon_path: "./telemetry-daemon".to_string(), // TODO: Make configurable
            current_pid: None,
            state: DaemonState::Unknown,
            process_stats: ProcessStats::default(),
            last_start_attempt: std::time::Instant::now(),
        })
    }

    pub async fn update_status(&mut self) {
        let mut system = self.system.lock().await;
        system.refresh_all();

        // Try to find the daemon process
        self.current_pid = None;
        for (pid, process) in system.processes() {
            if process.name().contains(&self.daemon_name) {
                self.current_pid = Some(pid.as_u32());
                break;
            }
        }

        // Update state based on PID
        self.state = match self.current_pid {
            Some(pid) => {
                if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
                    self.process_stats = ProcessStats {
                        pid: Some(pid),
                        cpu_usage: process.cpu_usage(),
                        memory_usage: process.memory() / 1024, // Convert to KB
                        virtual_memory: process.virtual_memory() / 1024, // Convert to KB
                        thread_count: 0, // TODO: Implement thread count properly
                        start_time: process.start_time(),
                        uptime_seconds: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            .saturating_sub(process.start_time()),
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

        // Start the daemon process
        let child = Command::new(&self.daemon_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to start daemon process: {}", e))?;

        self.current_pid = Some(child.id());
        Ok(())
    }

    pub fn stop_daemon(&mut self) -> Result<()> {
        let pid = self.current_pid.ok_or_else(|| anyhow!("No daemon process running"))?;

        self.state = DaemonState::Stopping;

        // Send SIGTERM first
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }

        // TODO: Implement graceful shutdown timeout and SIGKILL fallback
        Ok(())
    }

    pub fn get_state(&self) -> DaemonState {
        self.state
    }

    pub fn get_process_stats(&self) -> &ProcessStats {
        &self.process_stats
    }
}