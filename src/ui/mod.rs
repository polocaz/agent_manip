use crate::db::{Column, Database, QueryResult};
use crate::error::{validate_log_path, LogManagerError};
use crate::log_reader::{LogEntry, LogLevel, LogReader};
use crate::service::{ServiceManager, ServiceStatus};
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
    auto_scroll: bool,
    auto_update: bool,
    auto_update_interval: u32,
    // TODO: This field will be used for database operations in future implementations
    // Keep it for now as it's part of the core functionality
    db: Arc<Mutex<Database>>,
    selected_tab: Tab,
    last_update: std::time::Instant,
    log_entries: Vec<String>,
    log_order_earliest: bool,
    log_reader: Arc<Mutex<LogReader>>,
    log_path: String,
    log_path_input: String,
    file_load_status: FileLoadStatus,
    status_timeout: Duration,
    search_query: String,
    // Database tab fields
    db_path_input: String,
    db_readonly: bool,
    db_connected: bool,
    sql_query: String,
    query_result: Option<QueryResult>,
    db_tables: Vec<String>,
    selected_table: Option<String>,
    // Agent status
    service_manager: Arc<Mutex<Box<dyn ServiceManager>>>,
    service_status: ServiceStatus,
    service_last_check: std::time::Instant,
    service_logs: Vec<String>,
}

#[derive(PartialEq)]
enum Tab {
    Logs,
    Database,
    Agent,
    Settings,
}

impl AgentManagerApp {
    pub fn new(
        db: Database,
        log_reader: LogReader,
        service_manager: Arc<Mutex<Box<dyn ServiceManager>>>,
    ) -> Self {
        Self {
            auto_scroll: false,
            auto_update: true,
            auto_update_interval: 5,
            db: Arc::new(Mutex::new(db)),
            last_update: std::time::Instant::now(),
            log_entries: Vec::new(),
            log_reader: Arc::new(Mutex::new(log_reader)),
            log_order_earliest: true,
            log_path: String::new(),
            log_path_input: String::new(),
            file_load_status: FileLoadStatus::default(),
            search_query: String::new(),
            selected_tab: Tab::Logs,
            status_timeout: Duration::from_secs(5),
            // Database tab fields
            db_path_input: String::new(),
            db_readonly: true,
            db_connected: false,
            sql_query: String::new(),
            query_result: None,
            db_tables: Vec::new(),
            selected_table: None,
            // Service
            service_manager,
            service_status: ServiceStatus::Unknown,
            service_last_check: std::time::Instant::now(),
            service_logs: Vec::new(),
        }
    }

    /// Appends new log entries to the existing ones   
    pub fn update_logs(&mut self) {
        if let Ok(mut reader) = self.log_reader.lock() {
            let new_entries = reader.read_new_entries();

            if !new_entries.is_empty() {
                if self.log_order_earliest {
                    // If showing earliest first, append to the end
                    self.log_entries.extend(new_entries);
                } else {
                    // If showing latest first, prepend to the beginning
                    let mut new_entries = new_entries;
                    new_entries.reverse(); // Reverse so newest are first
                    new_entries.extend(self.log_entries.drain(..));
                    self.log_entries = new_entries;
                }
            }
        }
    }

    /// Reloads all log entries (used when changing file or order)
    pub fn reload_logs(&mut self) {
        if let Ok(mut reader) = self.log_reader.lock() {
            // Reset the reader's line counter
            reader.last_line_read = 0;

            // Load all entries fresh
            self.log_entries = reader.read_latest_entries(10000, self.log_order_earliest);
        }
    }

    fn flip_log_lines(&mut self) {
        self.log_order_earliest = !self.log_order_earliest;
        // Need to reload all logs when flipping order
        self.reload_logs();
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

    fn connect_to_database(&mut self) -> Result<(), String> {
        let path = self.db_path_input.trim();
        if path.is_empty() {
            return Err("Database path cannot be empty".to_string());
        }

        // Create a new database connection
        let db_result = if self.db_readonly {
            Database::new_readonly(path)
        } else {
            Database::new(path)
        };

        match db_result {
            Ok(new_db) => {
                // Update the database connection
                self.db = Arc::new(Mutex::new(new_db));
                self.db_connected = true;

                // Update the list of tables
                self.update_table_list();

                Ok(())
            }
            Err(e) => Err(format!("Failed to connect to database: {}", e)),
        }
    }

    fn update_table_list(&mut self) {
        if let Ok(db) = self.db.lock() {
            if let Ok(tables) = db.list_tables() {
                self.db_tables = tables;
                // Reset selected table
                self.selected_table = None;
            }
        }
    }

    fn execute_sql_query(&mut self) -> Result<(), String> {
        if !self.db_connected {
            return Err("Not connected to a database".to_string());
        }

        let query = self.sql_query.trim();
        if query.is_empty() {
            return Err("SQL query cannot be empty".to_string());
        }

        if let Ok(db) = self.db.lock() {
            match db.execute_query(query) {
                Ok(result) => {
                    self.query_result = Some(result);
                    Ok(())
                }
                Err(e) => Err(format!("Query execution failed: {}", e)),
            }
        } else {
            Err("Failed to access database".to_string())
        }
    }

    fn generate_schema_query(&mut self) -> Result<(), String> {
        if let Some(table) = &self.selected_table {
            self.sql_query = format!("PRAGMA table_info({})", table);
            self.execute_sql_query()
        } else {
            Err("No table selected".to_string())
        }
    }

    fn generate_select_query(&mut self) -> Result<(), String> {
        if let Some(table) = &self.selected_table {
            self.sql_query = format!("SELECT * FROM {} LIMIT 100", table);
            self.execute_sql_query()
        } else {
            Err("No table selected".to_string())
        }
    }

    /// SERVICE
    /// Update the service status
    fn update_service_status(&mut self) {
        if let Ok(manager) = self.service_manager.lock() {
            if let Ok(status) = manager.get_status() {
                self.service_status = status;
            }
        }
    }

    /// Start the service
    fn start_service(&mut self) -> Result<(), String> {
        if let Ok(manager) = self.service_manager.lock() {
            manager.start_service().map_err(|e| e.to_string())
        } else {
            Err("Failed to access service manager".to_string())
        }
    }

    /// Stop the service
    fn stop_service(&mut self) -> Result<(), String> {
        if let Ok(manager) = self.service_manager.lock() {
            manager.stop_service().map_err(|e| e.to_string())
        } else {
            Err("Failed to access service manager".to_string())
        }
    }

    /// Restart the service
    fn restart_service(&mut self) -> Result<(), String> {
        if let Ok(manager) = self.service_manager.lock() {
            manager.restart_service().map_err(|e| e.to_string())
        } else {
            Err("Failed to access service manager".to_string())
        }
    }

    /// Get service logs
    fn update_service_logs(&mut self) {
        if let Ok(manager) = self.service_manager.lock() {
            if let Ok(logs) = manager.get_logs(100) {
                self.service_logs = logs;
            }
        }
    }
}

impl eframe::App for AgentManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check service status periodically
        let now = std::time::Instant::now();
        if now.duration_since(self.service_last_check) >= Duration::from_secs(5) {
            self.update_service_status();
            if self.selected_tab == Tab::Agent {
                self.update_service_logs();
            }
            self.service_last_check = now;
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Logs").clicked() {
                    self.selected_tab = Tab::Logs;
                }
                if ui.button("Database").clicked() {
                    self.selected_tab = Tab::Database;
                }
                if ui.button("Agent").clicked() {
                    self.selected_tab = Tab::Agent;
                    // Update service status when switching to Agent tab
                    self.update_service_status();
                    self.update_service_logs();
                }
                if ui.button("Settings").clicked() {
                    self.selected_tab = Tab::Settings;
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.selected_tab {
            Tab::Logs => self.render_logs_tab(ui),
            Tab::Database => self.render_database_tab(ui),
            Tab::Agent => self.render_agent_tab(ui),
            Tab::Settings => self.render_settings_tab(ui),
        });
    }
}

impl AgentManagerApp {
    fn render_logs_tab(&mut self, ui: &mut egui::Ui) {
        // Debug log to show that auto-scroll is working
        println!("Auto-scroll: {0}", self.auto_scroll);

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

            if ui.button("Refresh").clicked() {
                self.reload_logs();
            }

            if ui.button("Flip order").clicked() {
                self.flip_log_lines();
            }

            // Add auto-scroll toggle
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");

            // Add auto-update interval slider
            ui.add(
                egui::Slider::new(&mut self.auto_update_interval, 1..=60)
                    .text("Update interval (s)"),
            );

            // Add auto-update toggle
            ui.checkbox(&mut self.auto_update, "Auto-update");
        });

        self.render_file_load_status(ui);

        // Auto-update and manual refresh controls
        if self.auto_update {
            let now = std::time::Instant::now();
            if now.duration_since(self.last_update) >= Duration::from_secs(5) {
                self.update_logs();
                self.last_update = now;
            }
        }

        // Search bar
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.search_query);
        });

        // Filter log entries based on search query
        let filtered_entries: Vec<&String> = if self.search_query.is_empty() {
            self.log_entries.iter().collect()
        } else {
            self.log_entries
                .iter()
                .filter(|entry| entry.contains(&self.search_query))
                .collect()
        };

        // Show visible rows TODO: Add formatting to log output
        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        let total_rows = filtered_entries.len();
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                for row in row_range {
                    ui.label(filtered_entries[row]);
                }
            });
    }

    fn render_database_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Database Management");

        // Database connection section
        ui.horizontal(|ui| {
            ui.label("Database path:");
            ui.text_edit_singleline(&mut self.db_path_input);

            if ui.button("Browse").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SQLite database", &["db", "sqlite", "sqlite3"])
                    .set_directory("./")
                    .pick_file()
                {
                    self.db_path_input = path.display().to_string();
                }
            }

            ui.checkbox(&mut self.db_readonly, "Read-only");

            if ui
                .button(if self.db_connected {
                    "Disconnect"
                } else {
                    "Connect"
                })
                .clicked()
            {
                if self.db_connected {
                    // Disconnect
                    self.db_connected = false;
                    self.query_result = None;
                    self.db_tables.clear();
                    self.selected_table = None;

                    self.file_load_status = FileLoadStatus {
                        message: "Disconnected from database".to_string(),
                        level: StatusLevel::Success,
                        timestamp: Some(std::time::Instant::now()),
                    };
                } else {
                    // Connect
                    match self.connect_to_database() {
                        Ok(_) => {
                            self.file_load_status = FileLoadStatus {
                                message: format!("Connected to database: {}", self.db_path_input),
                                level: StatusLevel::Success,
                                timestamp: Some(std::time::Instant::now()),
                            };
                        }
                        Err(e) => {
                            self.file_load_status = FileLoadStatus {
                                message: e,
                                level: StatusLevel::Error,
                                timestamp: Some(std::time::Instant::now()),
                            };
                        }
                    }
                }
            }
        });

        self.render_file_load_status(ui);

        if !self.db_connected {
            ui.label("Connect to a database to execute queries");
            return;
        }

        // Database tables selection
        ui.horizontal(|ui| {
            ui.label("Tables:");
            egui::ComboBox::new("tables_combo", "")
                .selected_text(self.selected_table.as_deref().unwrap_or("Select a table"))
                .show_ui(ui, |ui| {
                    for table in &self.db_tables {
                        ui.selectable_value(&mut self.selected_table, Some(table.clone()), table);
                    }
                });

            if ui.button("Refresh").clicked() {
                self.update_table_list();
            }

            if ui.button("Schema").clicked() {
                if let Err(e) = self.generate_schema_query() {
                    self.file_load_status = FileLoadStatus {
                        message: e,
                        level: StatusLevel::Error,
                        timestamp: Some(std::time::Instant::now()),
                    };
                }
            }

            if ui.button("Select *").clicked() {
                if let Err(e) = self.generate_select_query() {
                    self.file_load_status = FileLoadStatus {
                        message: e,
                        level: StatusLevel::Error,
                        timestamp: Some(std::time::Instant::now()),
                    };
                }
            }
        });

        // SQL Query section
        ui.group(|ui| {
            ui.label("SQL Query");

            let query_editor = egui::TextEdit::multiline(&mut self.sql_query)
                .desired_rows(5)
                .desired_width(f32::INFINITY);
            ui.add(query_editor);

            ui.horizontal(|ui| {
                if ui.button("Execute").clicked() {
                    match self.execute_sql_query() {
                        Ok(_) => {
                            if let Some(result) = &self.query_result {
                                self.file_load_status = FileLoadStatus {
                                    message: format!(
                                        "Query executed successfully. {} rows returned.",
                                        result.affected_rows
                                    ),
                                    level: StatusLevel::Success,
                                    timestamp: Some(std::time::Instant::now()),
                                };
                            }
                        }
                        Err(e) => {
                            self.file_load_status = FileLoadStatus {
                                message: e,
                                level: StatusLevel::Error,
                                timestamp: Some(std::time::Instant::now()),
                            };
                        }
                    }
                }

                if ui.button("Clear").clicked() {
                    self.sql_query.clear();
                    self.query_result = None;
                }
            });
        });

        // Query Results section
        ui.group(|ui| {
            ui.heading("Query Results");

            if let Some(result) = &self.query_result {
                if result.columns.is_empty() {
                    ui.label("No results to display");
                    return;
                }

                ui.label(format!("Rows: {}", result.affected_rows));

                // Table layout
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        // Setup a grid for the results
                        egui::Grid::new("query_results_grid")
                            .striped(true)
                            .spacing([5.0, 2.0])
                            .min_col_width(100.0)
                            .show(ui, |ui| {
                                // Header row
                                for col in &result.columns {
                                    ui.label(egui::RichText::new(&col.name).strong());
                                }
                                ui.end_row();

                                // Data rows
                                for row in &result.rows {
                                    for cell in row {
                                        ui.label(cell);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            } else {
                ui.label("Execute a query to see results");
            }
        });
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

    fn render_agent_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Agent Service Management");

        // Service status display
        ui.horizontal(|ui| {
            ui.label("Current Status:");
            let status_text = match self.service_status {
                ServiceStatus::Running => {
                    egui::RichText::new("Running").color(egui::Color32::GREEN)
                }
                ServiceStatus::Stopped => egui::RichText::new("Stopped").color(egui::Color32::RED),
                ServiceStatus::Unknown => {
                    egui::RichText::new("Unknown").color(egui::Color32::YELLOW)
                }
            };
            ui.label(status_text);
        });

        // Service control buttons
        ui.horizontal(|ui| {
            if ui.button("Start").clicked() {
                if let Err(e) = self.start_service() {
                    // Show error
                    self.file_load_status = FileLoadStatus {
                        message: format!("Failed to start service: {}", e),
                        level: StatusLevel::Error,
                        timestamp: Some(std::time::Instant::now()),
                    };
                } else {
                    // Show success
                    self.file_load_status = FileLoadStatus {
                        message: "Service started successfully".to_string(),
                        level: StatusLevel::Success,
                        timestamp: Some(std::time::Instant::now()),
                    };
                    // Update status
                    self.update_service_status();
                }
            }

            if ui.button("Stop").clicked() {
                if let Err(e) = self.stop_service() {
                    // Show error
                    self.file_load_status = FileLoadStatus {
                        message: format!("Failed to stop service: {}", e),
                        level: StatusLevel::Error,
                        timestamp: Some(std::time::Instant::now()),
                    };
                } else {
                    // Show success
                    self.file_load_status = FileLoadStatus {
                        message: "Service stopped successfully".to_string(),
                        level: StatusLevel::Success,
                        timestamp: Some(std::time::Instant::now()),
                    };
                    // Update status
                    self.update_service_status();
                }
            }

            if ui.button("Restart").clicked() {
                if let Err(e) = self.restart_service() {
                    // Show error
                    self.file_load_status = FileLoadStatus {
                        message: format!("Failed to restart service: {}", e),
                        level: StatusLevel::Error,
                        timestamp: Some(std::time::Instant::now()),
                    };
                } else {
                    // Show success
                    self.file_load_status = FileLoadStatus {
                        message: "Service restarted successfully".to_string(),
                        level: StatusLevel::Success,
                        timestamp: Some(std::time::Instant::now()),
                    };
                    // Update status
                    self.update_service_status();
                }
            }

            if ui.button("Refresh").clicked() {
                self.update_service_status();
                self.update_service_logs();
            }
        });

        self.render_file_load_status(ui);

        // Service logs
        ui.heading("Service Logs");
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for log in &self.service_logs {
                    ui.label(log);
                }
            });
    }
}
