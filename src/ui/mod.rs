use crate::db::Database;
use crate::error::{validate_log_path, LogManagerError};
use crate::log_reader::{LogEntry, LogLevel, LogReader};
use eframe::egui::{self, Grid, Layout, ScrollArea};
use std::sync::{Arc, Mutex};
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
    // TODO: This field will be used for database operations in future implementations
    // Keep it for now as it's part of the core functionality
    db: Arc<Mutex<Database>>,
    log_reader: Arc<Mutex<LogReader>>,
    log_entries: Vec<LogEntry>,
    selected_tab: Tab,
    log_path: String,
    log_path_input: String,
    file_load_status: FileLoadStatus,
    status_timeout: Duration,
    last_update: std::time::Instant,
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
            last_update: std::time::Instant::now(),
        }
    }

    /// Updates the log entries from the log reader
    pub fn update_logs(&mut self) {
        if let Ok(reader) = self.log_reader.lock() {
            self.log_entries = reader.read_latest_entries(100);
        }
    }

    fn update_log_file(&mut self, new_path: &str) {
        // Validate the new path
        if let Err(e) = validate_log_path(new_path) {
            self.file_load_status = FileLoadStatus {
                message: format!("Failed to update log file path: {e}"),
                level: StatusLevel::Error,
                timestamp: Some(std::time::Instant::now()),
            };
            return;
        }

        let result = match self.log_reader.lock() {
            Ok(mut reader) => reader.change_log_path(new_path),
            Err(e) => Err(LogManagerError::ReadError(format!(
                "Failed to update log file path to {new_path}: {e}"
            ))
            .into()),
        };

        match result {
            Ok(()) => {
                self.file_load_status = FileLoadStatus {
                    message: format!("Successfully loaded log file: {new_path}"),
                    level: StatusLevel::Success,
                    timestamp: Some(std::time::Instant::now()),
                };
                self.log_path = new_path.to_string();
                // Refresh log entries
                let () = self.update_logs();
            }
            Err(e) => {
                let (message, level) = match e.downcast_ref::<LogManagerError>() {
                    Some(LogManagerError::FileNotFound(path)) => {
                        (format!("Log file not found: {path}"), StatusLevel::Error)
                    }
                    Some(LogManagerError::PermissionDenied(path)) => (
                        format!("Permission denied for file: {path}"),
                        StatusLevel::Error,
                    ),
                    Some(LogManagerError::InvalidPath(path)) => {
                        (format!("Invalid file path: {path}"), StatusLevel::Warning)
                    }
                    _ => (format!("Failed to load log file: {e}"), StatusLevel::Error),
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

        egui::CentralPanel::default().show(ctx, |ui| match self.selected_tab {
            Tab::Logs => self.render_logs_tab(ui),
            Tab::Database => self.render_database_tab(ui),
            Tab::Settings => self.render_settings_tab(ui),
        });
    }
}

impl AgentManagerApp {
    fn render_logs_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Log Viewer");

        // File controls in top panel
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.text_edit_singleline(&mut self.log_path_input);
            if ui.button("Load").clicked() {
                let path = self.log_path_input.clone();
                self.update_log_file(&path);
            }

            if ui.button("Browse").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Log files", &["log"])
                    .set_directory("./")
                    .pick_file()
                {
                    self.log_path_input = path.display().to_string();
                }
            }
        });

        self.render_file_load_status(ui);

        // Auto-update and manual refresh controls
        let now = std::time::Instant::now();
        if now.duration_since(self.last_update) >= Duration::from_secs(5) {
            self.update_logs();
            self.last_update = now;
        }

        if ui.button("Refresh").clicked() {
            self.update_logs();
        }

        // Create scrollable area that fills available space
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                Grid::new("log_grid")
                    .striped(true)
                    .spacing([2.0, 2.0])
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        // Headers
                        ui.style_mut().spacing.item_spacing.x = 10.0;

                        ui.scope(|ui| {
                            ui.set_min_width(160.0);
                            ui.strong("Timestamp");
                        });

                        ui.scope(|ui| {
                            ui.set_min_width(100.0);
                            ui.strong("Component");
                        });

                        ui.scope(|ui| {
                            ui.set_min_width(50.0);
                            ui.strong("Line");
                        });

                        ui.scope(|ui| {
                            ui.set_min_width(50.0);
                            ui.strong("Level");
                        });

                        ui.scope(|ui| {
                            ui.set_min_width(80.0);
                            ui.strong("Thread Name");
                        });

                        ui.scope(|ui| {
                            ui.set_min_width(200.0);
                            ui.strong("Message");
                        });

                        ui.end_row();

                        // Log entries
                        for entry in &self.log_entries {
                            ui.scope(|ui| {
                                ui.set_min_width(160.0);
                                ui.label(entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string());
                            });

                            ui.scope(|ui| {
                                ui.set_min_width(100.0);
                                ui.label(&entry.component);
                            });

                            ui.scope(|ui| {
                                ui.set_min_width(50.0);
                                ui.label(format!("({})", entry.line_number));
                            });

                            ui.scope(|ui| {
                                ui.set_min_width(50.0);
                                let level_text = match entry.level {
                                    LogLevel::Info => egui::RichText::new("Info")
                                        .color(egui::Color32::from_rgb(100, 200, 100)),
                                    LogLevel::Warning => {
                                        egui::RichText::new("Warn").color(egui::Color32::YELLOW)
                                    }
                                    LogLevel::Error => {
                                        egui::RichText::new("Error").color(egui::Color32::RED)
                                    }
                                    _ => egui::RichText::new(entry.level.to_string()),
                                };
                                ui.label(level_text);
                            });

                            ui.scope(|ui| {
                                ui.set_min_width(80.0);
                                ui.label(&entry.thread_name);
                            });

                            ui.scope(|ui| {
                                ui.set_min_width(200.0);
                                ui.label(&entry.message);
                            });

                            ui.end_row();
                        }
                    });
            });
    }

    fn render_database_tab(&self, ui: &mut egui::Ui) {
        // TODO: Will use self.db for database operations in future implementation
        // Use self to get rid of the warning
        let _ = self.db;
        ui.heading("Database Management");
        // Add database management UI here
    }

    fn render_settings_tab(&self, ui: &mut egui::Ui) {
        // TODO: Will use instance data for settings in future implementation
        // Use self to get rid of the warning
        let _ = self;
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
