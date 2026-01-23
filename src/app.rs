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
    Settings,
}

impl Tab {
    pub fn next(&self) -> Self {
        match self {
            Tab::Overview => Tab::Resources,
            Tab::Resources => Tab::Network,
            Tab::Network => Tab::Logs,
            Tab::Logs => Tab::Settings,
            Tab::Settings => Tab::Overview,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Tab::Overview => Tab::Settings,
            Tab::Resources => Tab::Overview,
            Tab::Network => Tab::Resources,
            Tab::Logs => Tab::Network,
            Tab::Settings => Tab::Logs,
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
            KeyCode::F(5) => self.current_tab = Tab::Settings,
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