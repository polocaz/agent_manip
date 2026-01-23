use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::app::{App, Tab};

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.size();

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title bar
            Constraint::Length(3), // Tabs
            Constraint::Min(1),    // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(size);

    // Draw title
    draw_title(f, chunks[0]);

    // Draw tabs
    draw_tabs(f, chunks[1], app);

    // Draw main content based on current tab
    draw_main_content(f, chunks[2], app);

    // Draw status bar
    draw_status_bar(f, chunks[3], app);
}

fn draw_title(f: &mut Frame, area: Rect) {
    let title = Paragraph::new("LsiAgent Manager")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(title, area);
}

fn draw_tabs(f: &mut Frame, area: Rect, app: &App) {
    let tab_titles = vec!["Overview", "Resources", "Network", "Logs", "Settings"];

    let tabs = Tabs::new(tab_titles)
        .select(match app.current_tab {
            Tab::Overview => 0,
            Tab::Resources => 1,
            Tab::Network => 2,
            Tab::Logs => 3,
            Tab::Settings => 4,
        })
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("Tabs"));

    f.render_widget(tabs, area);
}

fn draw_main_content(f: &mut Frame, area: Rect, app: &App) {
    match app.current_tab {
        Tab::Overview => draw_overview(f, area, app),
        Tab::Resources => draw_resources(f, area, app),
        Tab::Network => draw_network(f, area, app),
        Tab::Logs => draw_logs(f, area, app),
        Tab::Settings => draw_settings(f, area, app),
    }
}

fn draw_overview(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Status summary
            Constraint::Length(5), // Key metrics
            Constraint::Min(1),    // Recent events
        ])
        .split(area);

    // Status summary
    let daemon_state = app.daemon_manager.get_state();
    let state_color = match daemon_state {
        crate::daemon::DaemonState::Running => Color::Green,
        crate::daemon::DaemonState::Stopped => Color::Red,
        _ => Color::Yellow,
    };

    let stats = app.daemon_manager.get_process_stats();
    let status_text = if let Some(pid) = stats.pid {
        format!("Daemon Status: {} (PID: {}, Process: {})", daemon_state, pid, app.daemon_manager.get_process_name())
    } else {
        format!("Daemon Status: {} (Looking for: {})", daemon_state, app.daemon_manager.get_process_name())
    };
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(state_color).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("Status"));

    f.render_widget(status, chunks[0]);

    // Key metrics
    let stats = app.daemon_manager.get_process_stats();
    let conn_status = app.network_monitor.get_connection_status();
    let net_stats = app.network_monitor.get_network_stats();

    let metrics_text = vec![
        Line::from(vec![
            Span::styled("CPU: ", Style::default().fg(Color::White)),
            Span::styled(format!("{:.1}%", stats.cpu_usage), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Memory: ", Style::default().fg(Color::White)),
            Span::styled(format_memory(stats.memory_usage), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Connection: ", Style::default().fg(Color::White)),
            Span::styled(
                if conn_status.is_connected { "Connected" } else { "Disconnected" },
                Style::default().fg(if conn_status.is_connected { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Data Flow: ", Style::default().fg(Color::White)),
            Span::styled(
                if net_stats.data_flow_active { "Active" } else { "Inactive" },
                Style::default().fg(if net_stats.data_flow_active { Color::Green } else { Color::Red }),
            ),
        ]),
    ];

    let metrics = Paragraph::new(metrics_text)
        .block(Block::default().borders(Borders::ALL).title("Key Metrics"));

    f.render_widget(metrics, chunks[1]);

    // Recent events - show systemctl status if available
    let events_text = if app.daemon_manager.is_using_systemctl() {
        match app.daemon_manager.get_service_status() {
            Ok(status) => {
                // Show the last few lines of systemctl status
                let lines: Vec<&str> = status.lines().collect();
                let recent_lines: Vec<String> = lines.iter().rev().take(5).rev().map(|s| s.to_string()).collect();
                recent_lines.join("\n")
            }
            Err(e) => format!("Failed to get systemctl status: {}", e),
        }
    } else {
        "systemctl not available - using direct process management".to_string()
    };

    let events = Paragraph::new(events_text)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title("Service Status"));

    f.render_widget(events, chunks[2]);
}

fn draw_resources(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // CPU & System Load
            Constraint::Length(3), // Memory
            Constraint::Length(3), // I/O Stats
            Constraint::Length(5), // Process Details
            Constraint::Min(1),    // Additional Info
        ])
        .split(area);

    let stats = app.daemon_manager.get_process_stats();

    // CPU usage and System Load
    let cpu_load_text = format!("CPU: {:.1}% | System Load: {:.2}",
        stats.cpu_usage,
        stats.system_load_avg
    );
    let cpu_load = Paragraph::new(cpu_load_text)
        .block(Block::default().borders(Borders::ALL).title("CPU & Load"));

    f.render_widget(cpu_load, chunks[0]);

    // Memory usage (Process and System)
    let memory_text = format!("Process: {} | Virtual: {}\nSystem: {} / {}",
        format_memory(stats.memory_usage),
        format_memory(stats.virtual_memory),
        format_memory(stats.system_memory_used),
        format_memory(stats.system_memory_total)
    );
    let memory = Paragraph::new(memory_text)
        .block(Block::default().borders(Borders::ALL).title("Memory"));

    f.render_widget(memory, chunks[1]);

    // I/O Statistics
    let io_text = format!("Disk Read: {} | Write: {}\nNet RX: {} | TX: {}",
        format_bytes(stats.disk_read_bytes),
        format_bytes(stats.disk_write_bytes),
        format_bytes(stats.network_rx_bytes),
        format_bytes(stats.network_tx_bytes)
    );
    let io_stats = Paragraph::new(io_text)
        .block(Block::default().borders(Borders::ALL).title("I/O Stats"));

    f.render_widget(io_stats, chunks[2]);

    // Process Details
    let details_text = format!("Process: {}\nPID: {:?} | PPID: {:?}\nState: {} | Priority: {}\nThreads: {} | Files: {}\nUptime: {}s",
        app.daemon_manager.get_process_name(),
        stats.pid,
        stats.ppid,
        stats.state,
        stats.priority,
        stats.thread_count,
        stats.open_files,
        stats.uptime_seconds
    );
    let details = Paragraph::new(details_text)
        .block(Block::default().borders(Borders::ALL).title("Process Details"));

    f.render_widget(details, chunks[3]);

    // Additional Info
    let additional_text = format!("Start Time: {}\nContext Switches: {}\nPage Faults: {}",
        format_timestamp(stats.start_time),
        stats.context_switches,
        stats.page_faults
    );
    let additional = Paragraph::new(additional_text)
        .block(Block::default().borders(Borders::ALL).title("Additional Info"));

    f.render_widget(additional, chunks[4]);
}

fn draw_network(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Connection status
            Constraint::Length(4), // Traffic stats
            Constraint::Min(1),    // Details
        ])
        .split(area);

    let conn_status = app.network_monitor.get_connection_status();
    let net_stats = app.network_monitor.get_network_stats();

    // Connection status
    let conn_color = if conn_status.is_connected { Color::Green } else { Color::Red };
    let conn_text = format!("Status: {}\nEndpoint: {}\nDuration: {:.1}s",
        if conn_status.is_connected { "Connected" } else { "Disconnected" },
        conn_status.endpoint,
        conn_status.connection_duration.as_secs_f32()
    );
    let connection = Paragraph::new(conn_text)
        .style(Style::default().fg(conn_color))
        .block(Block::default().borders(Borders::ALL).title("Connection"));

    f.render_widget(connection, chunks[0]);

    // Traffic stats
    let traffic_text = format!("Sent: {} ({} packets)\nReceived: {} ({} packets)\nData Flow: {}",
        format_bytes(net_stats.bytes_sent),
        net_stats.packets_sent,
        format_bytes(net_stats.bytes_received),
        net_stats.packets_received,
        if net_stats.data_flow_active { "Active" } else { "Inactive" }
    );
    let traffic = Paragraph::new(traffic_text)
        .block(Block::default().borders(Borders::ALL).title("Network Traffic"));

    f.render_widget(traffic, chunks[1]);

    // Additional details
    let details = Paragraph::new("Network monitoring details will be displayed here...")
        .block(Block::default().borders(Borders::ALL).title("Network Details"));

    f.render_widget(details, chunks[2]);
}

fn draw_logs(f: &mut Frame, area: Rect, _app: &App) {
    let logs = Paragraph::new("Log viewer will be implemented here...\n\nUse arrow keys to scroll\nPress 'f' to filter logs")
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title("Logs"));

    f.render_widget(logs, area);
}

fn draw_settings(f: &mut Frame, area: Rect, _app: &App) {
    let settings = Paragraph::new("Settings will be implemented here...\n\n- Refresh rate\n- Alert thresholds\n- Daemon configuration\n- Network endpoints")
        .block(Block::default().borders(Borders::ALL).title("Settings"));

    f.render_widget(settings, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let management_type = if app.daemon_manager.is_using_systemctl() {
        "systemctl"
    } else {
        "direct"
    };

    let status_text = format!(
        " [F1-F5] Tabs [h/l/j/k] Navigate | [S] Start [X] Stop [R] Refresh [Q] Quit | Mode: {} | Refresh: {}ms ",
        management_type,
        app.refresh_rate.as_millis()
    );

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::White).bg(Color::Blue))
        .alignment(Alignment::Center);

    f.render_widget(status, area);
}

fn format_memory(kb: u64) -> String {
    if kb < 1024 {
        format!("{} KB", kb)
    } else if kb < 1024 * 1024 {
        format!("{:.1} MB", kb as f64 / 1024.0)
    } else {
        format!("{:.1} GB", kb as f64 / (1024.0 * 1024.0))
    }
}

fn format_timestamp(timestamp: u64) -> String {
    use chrono::{Local, TimeZone};
    
    if let Some(datetime) = Local.timestamp_opt(timestamp as i64, 0).single() {
        datetime.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        format!("Invalid timestamp: {}", timestamp)
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}