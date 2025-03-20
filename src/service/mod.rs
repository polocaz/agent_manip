use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

// Platform-specific modules
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Stopped,
    Unknown,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Running => write!(f, "Running"),
            ServiceStatus::Stopped => write!(f, "Stopped"),
            ServiceStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

pub struct ServiceConfig {
    pub service_name: String,
    pub executable_path: PathBuf,
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub restart_delay: Duration,
    pub max_restarts: u32,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            service_name: "agent_service".to_string(),
            executable_path: PathBuf::new(),
            args: Vec::new(),
            working_directory: None,
            restart_delay: Duration::from_secs(5),
            max_restarts: 3,
        }
    }
}

/// Core service trait that all platform implementations must implement
pub trait ServiceManager {
    /// Get the current status of the service
    fn get_status(&self) -> io::Result<ServiceStatus>;
    
    /// Start the service
    fn start_service(&self) -> io::Result<()>;
    
    /// Stop the service
    fn stop_service(&self) -> io::Result<()>;
    
    /// Restart the service
    fn restart_service(&self) -> io::Result<()> {
        self.stop_service()?;
        std::thread::sleep(Duration::from_secs(2)); // Wait for service to stop
        self.start_service()
    }
    
    /// Get the service configuration
    fn get_config(&self) -> &ServiceConfig;
    
    /// Update the service configuration
    fn update_config(&mut self, config: ServiceConfig) -> io::Result<()>;
    
    /// Get the service logs
    fn get_logs(&self, line_count: usize) -> io::Result<Vec<String>>;
}

/// Helper function to run a system command
pub fn run_command(command: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(command)
        .args(args)
        .output()?;
    
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let error = String::from_utf8_lossy(&output.stderr).to_string();
        Err(io::Error::new(io::ErrorKind::Other, error))
    }
}

/// Creates a ServiceManager instance for the current platform
pub fn create_service_manager(config: ServiceConfig) -> Box<dyn ServiceManager> {
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxServiceManager::new(config))
    }
    
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsServiceManager::new(config))
    }
    
    #[cfg(target_os = "macos")]
    {
        Box::new(MacOSServiceManager::new(config))
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        panic!("Unsupported platform");
    }
} 