mod db;
mod error;
mod log_reader;
mod ui;

use anyhow::Result;
use eframe::{NativeOptions, run_native};
use egui::ViewportBuilder;
use std::path::PathBuf;
use std::io::Write;

fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    // Initialize database
    let db_path = PathBuf::from("agent.db");
    let database = db::Database::new(&db_path)?;
    database.init_schema()?;

    // Initialize log reader
    // If linux, load from /var/opt/lsiagent/lsiagent1.log
    // If macOS, load from /Library/Application Support/Lakeside Software/lsiagent.log
    // If Windows, load from C:\ProgramData\Lakeside Software\lsiagent.log
    let mut log_path = match std::env::consts::OS {
        "linux" => PathBuf::from("/var/opt/lsiagent/lsiagent1.log"),
        "macos" => PathBuf::from("/Library/Application Support/Lakeside Software/lsiagent.log"),
        "windows" => PathBuf::from("C:\\ProgramData\\Lakeside Software\\lsiagent.log"),
        _ => PathBuf::from("lsiagent1.log"),
    };
    
    // If the log file does not exist, use a local one
    if !log_path.exists() {
        log_path = PathBuf::from("lsiagent1.log");
        // Create the file if it doesn't exist
        if !log_path.exists() {
            let mut file = std::fs::File::create(&log_path)?;
            // Add a sample log entry
            writeln!(file, "2024-02-20T12:00:00Z INFO Application started - Using local log file")?;
        }
    }

    let log_reader = log_reader::LogReader::new(&log_path)?;

    // Create the eframe application
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    // Run the UI with error handling
    run_native(
        "LsiAgent Manager",
        options,
        Box::new(|_cc| Box::new(ui::AgentManagerApp::new(database, log_reader))),
    ).map_err(|e| anyhow::anyhow!("Failed to run application: {}", e))?;

    Ok(())
} 