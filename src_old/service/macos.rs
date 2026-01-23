use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use super::{run_command, ServiceConfig, ServiceManager, ServiceStatus};

/// macOS implementation of the ServiceManager trait using launchd
pub struct MacOSServiceManager {
    config: ServiceConfig,
    plist_path: PathBuf,
    ctl_path: String,
}

impl MacOSServiceManager {
    pub fn new(config: ServiceConfig) -> Self {
        // Use ~/Library/LaunchDaemons for user-level services
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let plist_path =
            home_dir.join("Library/LaunchDaemons/com.lakesidesoftware.lsiagentd.plist");
        let ctl_path = "/Library/Application Support/Lakeside Software/lsiagentctl".to_string();

        Self {
            config,
            plist_path,
            ctl_path,
        }
    }

    /// Creates a launchd plist file
    fn create_plist_file(&self) -> io::Result<()> {
        let executable_path = self.config.executable_path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid executable path")
        })?;

        let working_dir = match &self.config.working_directory {
            Some(dir) => dir.to_str().unwrap_or("."),
            None => ".",
        };

        // Create program arguments array
        let mut program_args = Vec::new();
        program_args.push(format!("\t\t<string>{}</string>", executable_path));

        for arg in &self.config.args {
            program_args.push(format!("\t\t<string>{}</string>", arg));
        }

        let program_args_str = program_args.join("\n");

        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.{}</string>
    <key>ProgramArguments</key>
    <array>
{}
    </array>
    <key>WorkingDirectory</key>
    <string>{}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}/Library/Logs/{}.log</string>
    <key>StandardErrorPath</key>
    <string>{}/Library/Logs/{}.err.log</string>
</dict>
</plist>"#,
            self.config.service_name,
            program_args_str,
            working_dir,
            home_dir_str(),
            self.config.service_name,
            home_dir_str(),
            self.config.service_name
        );

        // Ensure the LaunchDaemons directory exists
        if let Some(parent) = self.plist_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::write(&self.plist_path, plist_content)?;

        Ok(())
    }

    /// Loads the service into launchd
    fn load_service(&self) -> io::Result<()> {
        let plist_path = self
            .plist_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid plist path"))?;

        run_command("launchctl", &["load", "-w", plist_path])?;

        Ok(())
    }

    /// Unloads the service from launchd
    fn unload_service(&self) -> io::Result<()> {
        let plist_path = self
            .plist_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid plist path"))?;

        run_command("launchctl", &["unload", "-w", plist_path])?;

        Ok(())
    }
}

impl ServiceManager for MacOSServiceManager {
    fn get_status(&self) -> io::Result<ServiceStatus> {
        let output = Command::new(self.ctl_path.clone())
            .args(["status"])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        // Print output for debugging
        println!("{}", stdout);

        if stdout.contains("running") {
            Ok(ServiceStatus::Running)
        } else if self.plist_path.exists() {
            Ok(ServiceStatus::Stopped)
        } else {
            Ok(ServiceStatus::Unknown)
        }
    }

    fn start_service(&self) -> io::Result<()> {
        // Todo - interact with plist directly and use launchctl instead

        // Start the service
        run_command(&self.ctl_path, &["start"])?;

        Ok(())
    }

    fn stop_service(&self) -> io::Result<()> {
        // Stop the service
        run_command(&self.ctl_path, &["stop"])?;

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

        // Update plist path
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        self.plist_path = home_dir
            .join("Library/LaunchDaemons")
            .join(format!("com.{}.plist", self.config.service_name));

        // Recreate plist file
        self.create_plist_file()?;

        // Restart if it was running
        if was_running {
            self.start_service()?;
        }

        Ok(())
    }

    fn get_logs(&self, line_count: usize) -> io::Result<Vec<String>> {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let log_path = home_dir
            .join("Library/Logs")
            .join(format!("{}.log", self.config.service_name));

        if !log_path.exists() {
            return Ok(Vec::new());
        }

        let log_path_str = log_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid log path"))?;

        let count_arg = format!("-n {}", line_count);

        let output = Command::new("tail")
            .args([&count_arg, log_path_str])
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

// Helper function to get home directory as string
fn home_dir_str() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .to_str()
        .unwrap_or(".")
        .to_string()
}
