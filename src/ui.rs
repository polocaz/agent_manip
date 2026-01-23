use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Tabs, Wrap},
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

    let status_text = format!("Daemon Status: {}", daemon_state);
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

    // Recent events placeholder
    let events = Paragraph::new("Recent events will be displayed here...")
        .block(Block::default().borders(Borders::ALL).title("Recent Events"));

    f.render_widget(events, chunks[2]);
}

fn draw_resources(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // CPU
            Constraint::Length(3), // Memory
            Constraint::Length(3), // Threads
            Constraint::Min(1),    // Details
        ])
        .split(area);

    let stats = app.daemon_manager.get_process_stats();

    // CPU usage gauge
    let cpu_gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("CPU Usage"))
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent((stats.cpu_usage / 100.0 * 100.0) as u16)
        .label(format!("{:.1}%", stats.cpu_usage));

    f.render_widget(cpu_gauge, chunks[0]);

    // Memory usage
    let memory_text = format!("Memory: {} | Virtual: {}",
        format_memory(stats.memory_usage),
        format_memory(stats.virtual_memory)
    );
    let memory = Paragraph::new(memory_text)
        .block(Block::default().borders(Borders::ALL).title("Memory"));

    f.render_widget(memory, chunks[1]);

    // Thread count
    let thread_text = format!("Threads: {}", stats.thread_count);
    let threads = Paragraph::new(thread_text)
        .block(Block::default().borders(Borders::ALL).title("Threads"));

    f.render_widget(threads, chunks[2]);

    // Additional details
    let details_text = format!("PID: {:?}\nUptime: {}s\nStart Time: {}",
        stats.pid,
        stats.uptime_seconds,
        stats.start_time
    );
    let details = Paragraph::new(details_text)
        .block(Block::default().borders(Borders::ALL).title("Process Details"));

    f.render_widget(details, chunks[3]);
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
    let status_text = format!(
        " [F1] Overview [F2] Resources [F3] Network [F4] Logs [F5] Settings | [S] Start [X] Stop [R] Refresh [Q] Quit | Refresh: {}ms ",
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