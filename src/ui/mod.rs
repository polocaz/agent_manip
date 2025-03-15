use eframe::egui;
use crate::db::Database;
use crate::error::{LogManagerError, validate_log_path};
use crate::log_reader::{LogReader, LogEntry};
use std::sync::{Arc, Mutex};
use anyhow::Result;
use std::time::Duration;
#[derive(Default)]
pub struct FileLoadStatus {
    message: String,
    level: StatusLevel,
    timestamp: Option<std::time::Instant>,
}

#[derive(Default)]
pub enum StatusLevel {
    #[default]
    None,
    Success,
    Warning,
    Error,
}

pub struct AgentManagerApp {
    db: Arc<Mutex<Database>>,
    log_reader: Arc<Mutex<LogReader>>,
    log_entries: Vec<LogEntry>,
    selected_tab: Tab,
    log_path: String,
    log_path_input: String,
    file_load_status: FileLoadStatus,
    status_timeout: Duration,
}

#[derive(PartialEq)]
enum Tab {
    Logs,
    Database,
    Settings,
}

impl AgentManagerApp {
    pub fn new(db: Database, log_reader: LogReader) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            log_reader: Arc::new(Mutex::new(log_reader)),
            log_entries: Vec::new(),
            selected_tab: Tab::Logs,
            log_path: String::new(),
            log_path_input: String::new(),
            file_load_status: FileLoadStatus::default(),
            status_timeout: Duration::from_secs(5),
        }
    }

    pub fn update_logs(&mut self) -> Result<()> {
        if let Ok(mut reader) = self.log_reader.lock() {
            self.log_entries = reader.read_latest_entries(100)?;
        }
        Ok(())
    }

    fn update_log_file(&mut self, new_path: &str) {
        // Validate the new path
        if let Err(e) = validate_log_path(new_path) {
            self.file_load_status = FileLoadStatus {
                message: format!("Failed to update log file path: {}", e),
                level: StatusLevel::Error,
                timestamp: Some(std::time::Instant::now()),
            };
            return;
        }

        let result = match self.log_reader.lock() {
            Ok(mut reader) => reader.change_log_path(new_path),
            Err(e) => Err(LogManagerError::ReadError(
                format!("Failed to update log file path to {}: {}", new_path, e)
            ).into()),
        };

        match result {
            Ok(_) => {
                self.file_load_status = FileLoadStatus {
                    message: format!("Successfully loaded log file: {}", new_path),
                    level: StatusLevel::Success,
                    timestamp: Some(std::time::Instant::now()),
                };
                self.log_path = new_path.to_string();
                // Refresh log entries
                let _ = self.update_logs();
            }
            Err(e) => {
                let (message, level) = match e.downcast_ref::<LogManagerError>() {
                    Some(LogManagerError::FileNotFound(path)) => (
                        format!("Log file not found: {}", path),
                        StatusLevel::Error
                    ),
                    Some(LogManagerError::PermissionDenied(path)) => (
                        format!("Permission denied for file: {}", path),
                        StatusLevel::Error
                    ),
                    Some(LogManagerError::InvalidPath(path)) => (
                        format!("Invalid file path: {}", path),
                        StatusLevel::Warning
                    ),
                    _ => (
                        format!("Failed to load log file: {}", e),
                        StatusLevel::Error
                    ),
                };

                self.file_load_status = FileLoadStatus {
                    message,
                    level,
                    timestamp: Some(std::time::Instant::now()),
                };
            }
        }
    }
}

impl eframe::App for AgentManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Logs").clicked() {
                    self.selected_tab = Tab::Logs;
                }
                if ui.button("Database").clicked() {
                    self.selected_tab = Tab::Database;
                }
                if ui.button("Settings").clicked() {
                    self.selected_tab = Tab::Settings;
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                Tab::Logs => self.render_logs_tab(ui),
                Tab::Database => self.render_database_tab(ui),
                Tab::Settings => self.render_settings_tab(ui),
            }
        });
    }
}

impl AgentManagerApp {
    fn render_logs_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Log Viewer");
        
        // TODO: Add filter by level
        // ui.horizontal(|ui| {
        //     ui.label("Filter by level:");
        //     ui.combo_box_with_label(&mut self.filter_level, &["All", "Debug", "Info", "Warn", "Error"]);
        // });

        // File choice/loading
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.text_edit_singleline(&mut self.log_path_input);
            if ui.button("Load").clicked() {
                let path = self.log_path_input.clone();
                self.update_log_file(&path);
            }
        });

        self.render_file_load_status(ui);

        if ui.button("Refresh").clicked() {
            let _ = self.update_logs();
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &self.log_entries {
                ui.horizontal(|ui| {
                    ui.label(entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string());
                    ui.label(&entry.level);
                    ui.label(&entry.message);
                });
            }
        });
    }

    fn render_database_tab(&self, ui: &mut egui::Ui) {
        ui.heading("Database Management");
        // Add database management UI here
    }

    fn render_settings_tab(&self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        // Add settings UI here
    }

    fn render_file_load_status(&mut self, ui: &mut egui::Ui) {
        if let Some(timestamp) = self.file_load_status.timestamp {
            if timestamp.elapsed() < self.status_timeout {
                let text = egui::RichText::new(&self.file_load_status.message);
                let text = match self.file_load_status.level {
                    StatusLevel::Success => text.color(egui::Color32::GREEN),
                    StatusLevel::Warning => text.color(egui::Color32::YELLOW),
                    StatusLevel::Error => text.color(egui::Color32::RED),
                    StatusLevel::None => text,
                };
                ui.label(text);
            } else {
                // Clear expired status
                self.file_load_status = FileLoadStatus::default();
            }
        }
    }
} 