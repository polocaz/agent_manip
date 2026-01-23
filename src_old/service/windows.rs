use std::io;
use std::path::PathBuf;
use std::process::Command;

use super::{ServiceConfig, ServiceManager, ServiceStatus, run_command};

/// Windows implementation of the ServiceManager trait using Windows Service Control Manager
pub struct WindowsServiceManager {
    config: ServiceConfig,
}

impl WindowsServiceManager {
    pub fn new(config: ServiceConfig) -> Self {
        Self {
            config,
        }
    }
    
    /// Creates a Windows service using sc.exe
    fn create_service(&self) -> io::Result<()> {
        let executable_path = self.config.executable_path
            .to_str()
            .ok_or_else(|| io::Error::new(
                io::ErrorKind::InvalidInput, 
                "Invalid executable path"
            ))?;
            
        let args = if self.config.args.is_empty() {
            String::new()
        } else {
            format!(" {}", self.config.args.join(" "))
        };
        
        let binpath = format!("\"{}{}\"", executable_path, args);
        
        // Check if service already exists
        let status = self.get_status()?;
        if status != ServiceStatus::Unknown {
            // Service exists, delete it first
            self.delete_service()?;
        }
        
        // Create service
        let output = Command::new("sc")
            .args(["create", &self.config.service_name, "binPath=", &binpath, "start=", "auto"])
            .output()?;
            
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(io::Error::new(io::ErrorKind::Other, error));
        }
        
        // Set description
        let desc = format!("Agent Service - {}", self.config.service_name);
        let _ = Command::new("sc")
            .args(["description", &self.config.service_name, &desc])
            .output()?;
            
        Ok(())
    }
    
    /// Deletes the Windows service
    fn delete_service(&self) -> io::Result<()> {
        let output = Command::new("sc")
            .args(["delete", &self.config.service_name])
            .output()?;
            
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(io::Error::new(io::ErrorKind::Other, error));
        }
        
        Ok(())
    }
}

impl ServiceManager for WindowsServiceManager {
    fn get_status(&self) -> io::Result<ServiceStatus> {
        let output = Command::new("sc")
            .args(["query", &self.config.service_name])
            .output()?;
            
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        
        // Service doesn't exist
        if stdout.contains("1060") || stdout.contains("The specified service does not exist") {
            return Ok(ServiceStatus::Unknown);
        }
        
        if stdout.contains("RUNNING") {
            Ok(ServiceStatus::Running)
        } else if stdout.contains("STOPPED") {
            Ok(ServiceStatus::Stopped)
        } else {
            Ok(ServiceStatus::Unknown)
        }
    }
    
    fn start_service(&self) -> io::Result<()> {
        // Check if service exists, create if not
        let status = self.get_status()?;
        if status == ServiceStatus::Unknown {
            self.create_service()?;
        }
        
        // Start service
        let output = Command::new("sc")
            .args(["start", &self.config.service_name])
            .output()?;
            
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(io::Error::new(io::ErrorKind::Other, error));
        }
        
        Ok(())
    }
    
    fn stop_service(&self) -> io::Result<()> {
        let status = self.get_status()?;
        if status == ServiceStatus::Running {
            let output = Command::new("sc")
                .args(["stop", &self.config.service_name])
                .output()?;
                
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(io::Error::new(io::ErrorKind::Other, error));
            }
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
        
        // Delete existing service
        if status != ServiceStatus::Unknown {
            self.delete_service()?;
        }
        
        // Update config
        self.config = config;
        
        // Recreate service
        self.create_service()?;
        
        // Restart if it was running
        if was_running {
            self.start_service()?;
        }
        
        Ok(())
    }
    
    fn get_logs(&self, line_count: usize) -> io::Result<Vec<String>> {
        let output = Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Get-WinEvent -FilterHashTable @{{Logname='Application'; ProviderName='{}'}} -MaxEvents {} | Format-Table -AutoSize -Wrap",
                    self.config.service_name, 
                    line_count
                )
            ])
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