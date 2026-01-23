use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use super::{ServiceConfig, ServiceManager, ServiceStatus, run_command};

/// Linux implementation of the ServiceManager trait using systemd
pub struct LinuxServiceManager {
    config: ServiceConfig,
    service_file_path: PathBuf,
}

impl LinuxServiceManager {
    pub fn new(config: ServiceConfig) -> Self {
        let service_file_path = PathBuf::from(format!(
            "/etc/systemd/system/{}.service", 
            config.service_name
        ));
        
        Self {
            config,
            service_file_path,
        }
    }
    
    /// Creates a systemd service file
    fn create_service_file(&self) -> io::Result<()> {
        let executable_path = self.config.executable_path
            .to_str()
            .ok_or_else(|| io::Error::new(
                io::ErrorKind::InvalidInput, 
                "Invalid executable path"
            ))?;
            
        let working_dir = match &self.config.working_directory {
            Some(dir) => dir.to_str().unwrap_or("."),
            None => ".",
        };
        
        let args = self.config.args.join(" ");
        
        let service_content = format!(
            "[Unit]
Description=Agent Service - {}
After=network.target

[Service]
Type=simple
ExecStart={} {}
WorkingDirectory={}
Restart=on-failure
RestartSec={}
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
",
            self.config.service_name,
            executable_path,
            args,
            working_dir,
            self.config.restart_delay.as_secs()
        );
        
        // Check if we have permission to write to systemd directory
        if !self.service_file_path.parent().map_or(false, |p| p.exists()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Cannot access systemd directory. Try running with sudo."
            ));
        }
        
        fs::write(&self.service_file_path, service_content)?;
        
        // Reload systemd to recognize the new service
        run_command("systemctl", &["daemon-reload"])?;
        
        Ok(())
    }
}

impl ServiceManager for LinuxServiceManager {
    fn get_status(&self) -> io::Result<ServiceStatus> {
        let service_name = format!("{}.service", self.config.service_name);
        let result = Command::new("systemctl")
            .args(["is-active", &service_name])
            .output()?;
            
        let status = String::from_utf8_lossy(&result.stdout).trim().to_string();
        
        match status.as_str() {
            "active" => Ok(ServiceStatus::Running),
            "inactive" | "failed" => Ok(ServiceStatus::Stopped),
            _ => Ok(ServiceStatus::Unknown),
        }
    }
    
    fn start_service(&self) -> io::Result<()> {
        // Check if service file exists, create if not
        if !self.service_file_path.exists() {
            self.create_service_file()?;
        }
        
        let service_name = format!("{}.service", self.config.service_name);
        run_command("systemctl", &["start", &service_name])?;
        
        Ok(())
    }
    
    fn stop_service(&self) -> io::Result<()> {
        let service_name = format!("{}.service", self.config.service_name);
        
        // Only try to stop if the service exists
        if self.service_file_path.exists() {
            run_command("systemctl", &["stop", &service_name])?;
        }
        
        Ok(())
    }
    
    fn get_config(&self) -> &ServiceConfig {
        &self.config
    }
    
    fn update_config(&mut self, config: ServiceConfig) -> io::Result<()> {
        // Check if service is running
        let status = self.get_status()?;
        let was_running = status == ServiceStatus::Running;
        
        // Stop service if it's running
        if was_running {
            self.stop_service()?;
        }
        
        // Update config
        self.config = config;
        self.service_file_path = PathBuf::from(format!(
            "/etc/systemd/system/{}.service", 
            self.config.service_name
        ));
        
        // Recreate service file
        self.create_service_file()?;
        
        // Restart if it was running
        if was_running {
            self.start_service()?;
        }
        
        Ok(())
    }
    
    fn get_logs(&self, line_count: usize) -> io::Result<Vec<String>> {
        let service_name = format!("{}.service", self.config.service_name);
        let count_arg = format!("-n{}", line_count);
        
        let output = Command::new("journalctl")
            .args(["-u", &service_name, &count_arg, "--no-pager"])
            .output()?;
            
        if output.status.success() {
            let logs = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(String::from)
                .collect();
            Ok(logs)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            Err(io::Error::new(io::ErrorKind::Other, error))
        }
    }
} 