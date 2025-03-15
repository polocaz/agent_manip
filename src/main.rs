mod db;
mod error;
mod log_reader;
mod ui;

use anyhow::Result;
use eframe::{NativeOptions, run_native};
use egui::ViewportBuilder;
use std::path::PathBuf;

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
    let log_path = match std::env::consts::OS {
        "linux" => PathBuf::from("/var/opt/lsiagent/lsiagent1.log"),
        "macos" => PathBuf::from("/Library/Application Support/Lakeside Software/lsiagent.log"),
        "windows" => PathBuf::from("C:\\ProgramData\\Lakeside Software\\lsiagent.log"),
        _ => PathBuf::from("lsiagent1.log"),
    };
    let log_reader = log_reader::LogReader::new(log_path)?;

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