use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::KeyCode;

use crate::daemon::DaemonManager;
use crate::network::NetworkMonitor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Resources,
    Network,
    Logs,
    Config,
    Settings,
}

impl Tab {
    pub fn next(&self) -> Self {
        match self {
            Tab::Overview => Tab::Resources,
            Tab::Resources => Tab::Network,
            Tab::Network => Tab::Logs,
            Tab::Logs => Tab::Config,
            Tab::Config => Tab::Settings,
            Tab::Settings => Tab::Overview,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Tab::Overview => Tab::Settings,
            Tab::Resources => Tab::Overview,
            Tab::Network => Tab::Resources,
            Tab::Logs => Tab::Network,
            Tab::Config => Tab::Logs,
            Tab::Settings => Tab::Config,
        }
    }
}

pub struct App {
    pub should_quit: bool,
    pub current_tab: Tab,
    pub daemon_manager: DaemonManager,
    pub network_monitor: NetworkMonitor,
    pub last_update: Instant,
    pub refresh_rate: Duration,
    pub config_scroll: u16, // Scroll position for config tab
    pub logs_scroll: u16,   // Scroll position for logs tab
    pub current_log_file: usize, // Current log file index (0-9)
    pub last_log_content_len: usize, // Track log content length for auto-scroll
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self {
            should_quit: false,
            current_tab: Tab::Overview,
            daemon_manager: DaemonManager::new()?,
            network_monitor: NetworkMonitor::new()?,
            last_update: Instant::now(),
            refresh_rate: Duration::from_millis(1000), // 1 second refresh
            config_scroll: 0,
            logs_scroll: 0,
            current_log_file: 0,
            last_log_content_len: 0,
        })
    }

    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => self.current_tab = self.current_tab.next(),
            KeyCode::BackTab => self.current_tab = self.current_tab.prev(),
            KeyCode::F(1) => self.current_tab = Tab::Overview,
            KeyCode::F(2) => self.current_tab = Tab::Resources,
            KeyCode::F(3) => self.current_tab = Tab::Network,
            KeyCode::F(4) => self.current_tab = Tab::Logs,
            KeyCode::F(5) => self.current_tab = Tab::Config,
            KeyCode::F(6) => self.current_tab = Tab::Settings,
            // Vim-style navigation
            KeyCode::Char('h') => self.current_tab = self.current_tab.prev(), // left/previous tab
            KeyCode::Char('l') => self.current_tab = self.current_tab.next(), // right/next tab
            KeyCode::Char('j') => self.current_tab = self.current_tab.next(), // down/next tab
            KeyCode::Char('k') => self.current_tab = self.current_tab.prev(), // up/previous tab
            KeyCode::Char('s') => {
                if let Err(e) = self.daemon_manager.start_daemon() {
                    eprintln!("Failed to start daemon: {}", e);
                }
            }
            KeyCode::Char('x') => {
                if let Err(e) = self.daemon_manager.stop_daemon() {
                    eprintln!("Failed to stop daemon: {}", e);
                }
            }
            KeyCode::Char('r') => {
                // Manual refresh
                self.last_update = Instant::now() - self.refresh_rate;
            }
            KeyCode::Up => {
                match self.current_tab {
                    Tab::Config => {
                        if self.config_scroll > 0 {
                            self.config_scroll = self.config_scroll.saturating_sub(1);
                        }
                    }
                    Tab::Logs => {
                        if self.logs_scroll > 0 {
                            self.logs_scroll = self.logs_scroll.saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                match self.current_tab {
                    Tab::Config => {
                        self.config_scroll += 1; // Will be clamped in the UI
                    }
                    Tab::Logs => {
                        self.logs_scroll += 1; // Will be clamped in the UI
                    }
                    _ => {}
                }
            }
            KeyCode::PageUp => {
                match self.current_tab {
                    Tab::Config => {
                        if self.config_scroll > 0 {
                            self.config_scroll = self.config_scroll.saturating_sub(10);
                        }
                    }
                    Tab::Logs => {
                        if self.logs_scroll > 0 {
                            self.logs_scroll = self.logs_scroll.saturating_sub(10);
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::PageDown => {
                match self.current_tab {
                    Tab::Config => {
                        self.config_scroll += 10; // Will be clamped in the UI
                    }
                    Tab::Logs => {
                        self.logs_scroll += 10; // Will be clamped in the UI
                    }
                    _ => {}
                }
            }
            KeyCode::Left => {
                if self.current_tab == Tab::Logs && self.current_log_file > 0 {
                    self.current_log_file -= 1;
                    self.logs_scroll = 0; // Reset scroll when switching files
                    self.last_log_content_len = 0; // Reset content tracking for new file
                }
            }
            KeyCode::Right => {
                if self.current_tab == Tab::Logs && self.current_log_file < 9 {
                    self.current_log_file += 1;
                    self.logs_scroll = 0; // Reset scroll when switching files
                    self.last_log_content_len = 0; // Reset content tracking for new file
                }
            }
            KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') | KeyCode::Char('5') | KeyCode::Char('6') | KeyCode::Char('7') | KeyCode::Char('8') | KeyCode::Char('9') => {
                if self.current_tab == Tab::Logs {
                    if let KeyCode::Char(c) = key.code {
                        if let Some(digit) = c.to_digit(10) {
                            self.current_log_file = digit as usize;
                            self.logs_scroll = 0; // Reset scroll when switching files
                            self.last_log_content_len = 0; // Reset content tracking for new file
                        }
                    }
                }
            }
            KeyCode::Char('0') => {
                if self.current_tab == Tab::Logs {
                    self.current_log_file = 0;
                    self.logs_scroll = 0; // Reset scroll when switching files
                    self.last_log_content_len = 0; // Reset content tracking for new file
                }
            }
            _ => {}
        }
    }

    pub async fn on_tick(&mut self) {
        if self.last_update.elapsed() >= self.refresh_rate {
            self.daemon_manager.update_status().await;
            self.network_monitor.update().await;
            self.last_update = Instant::now();
        }
    }
}