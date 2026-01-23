use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Frame,
    Terminal, backend::CrosstermBackend,
};
use anyhow::Result;

use crate::app::{App, Tab};

pub async fn show_startup_animation(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    // Clear the screen first
    terminal.clear()?;

    let frames = vec![
        // Frame 1: Header
        vec![
            Line::from(vec![Span::styled("[ S Y S T R A C K   S O F T W A R E   ( C ) 2000 ]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("> INITIALIZING MODULE...", Style::default().fg(Color::Green))]),
        ],
        // Frame 2: Loading message
        vec![
            Line::from(vec![Span::styled("[ S Y S T R A C K   S O F T W A R E   ( C ) 2000 ]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("> INITIALIZING MODULE...", Style::default().fg(Color::Green))]),
            Line::from(vec![Span::styled("> LOADING: LSI AGENT", Style::default().fg(Color::Green))]),
        ],
        // Frame 3: ASCII Art
        vec![
            Line::from(vec![Span::styled("[ S Y S T R A C K   S O F T W A R E   ( C ) 2000 ]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("> INITIALIZING MODULE...", Style::default().fg(Color::Green))]),
            Line::from(vec![Span::styled("> LOADING: LSI AGENT", Style::default().fg(Color::Green))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("  ██╗     ███████╗██╗     █████╗  ██████╗ ███████╗███╗   ██╗████████╗", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ██║     ██╔════╝██║    ██╔══██╗██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ██║     ███████╗██║    ███████║██║  ███╗█████╗  ██╔██╗ ██║   ██║   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ██║     ╚════██║██║    ██╔══██║██║   ██║██╔══╝  ██║╚██╗██║   ██║   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ███████╗███████║██║    ██║  ██║╚██████╔╝███████╗██║ ╚████║   ██║   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ╚══════╝╚══════╝╚═╝    ╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
        ],
        // Frame 4: Status
        vec![
            Line::from(vec![Span::styled("[ S Y S T R A C K   S O F T W A R E   ( C ) 2000 ]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("> INITIALIZING MODULE...", Style::default().fg(Color::Green))]),
            Line::from(vec![Span::styled("> LOADING: LSI AGENT", Style::default().fg(Color::Green))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("  ██╗     ███████╗██╗     █████╗  ██████╗ ███████╗███╗   ██╗████████╗", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ██║     ██╔════╝██║    ██╔══██╗██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ██║     ███████╗██║    ███████║██║  ███╗█████╗  ██╔██╗ ██║   ██║   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ██║     ╚════██║██║    ██╔══██║██║   ██║██╔══╝  ██║╚██╗██║   ██║   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ███████╗███████║██║    ██║  ██║╚██████╔╝███████╗██║ ╚████║   ██║   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled("  ╚══════╝╚══════╝╚═╝    ╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝   ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
            Line::from(vec![]),
            Line::from(vec![Span::styled("> STATUS: OPERATIONAL", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
        ],
    ];

    for (i, frame) in frames.iter().enumerate() {
        terminal.draw(|f| {
            let size = f.size();
            let paragraph = Paragraph::new(frame.clone())
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::NONE));
            f.render_widget(paragraph, size);
        })?;

        // Different delays for different frames
        let delay = match i {
            0 => 1500, // Header - longer delay
            1 => 1000, // Loading message
            2 => 2000, // ASCII art - longer to show the logo
            3 => 2000, // Final status - longer to show completion
            _ => 500,
        };
        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
    }

    // Final pause before main interface
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

    Ok(())
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.size();

    // Clear background to black for Pip-Boy aesthetic
    f.render_widget(
        ratatui::widgets::Clear,
        size,
    );

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Title bar with ASCII art
            Constraint::Length(3), // Tabs
            Constraint::Min(1),    // Main content
            Constraint::Length(2), // Status bar
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
    let title_text = vec![
        Line::from(vec![
            Span::styled("╔══════════════════════════════════════════════════════════════════════════════╗", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("║", Style::default().fg(Color::Green)),
            Span::styled("                          LSI AGENT MANAGEMENT TERMINAL                          ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("║", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("║", Style::default().fg(Color::Green)),
            Span::styled("                        [PIP-BOY INTERFACE v2.1.7]                        ", Style::default().fg(Color::Green)),
            Span::styled("║", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("╚══════════════════════════════════════════════════════════════════════════════╝", Style::default().fg(Color::Green)),
        ]),
    ];

    let title = Paragraph::new(title_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));

    f.render_widget(title, area);
}

fn draw_tabs(f: &mut Frame, area: Rect, app: &App) {
    let tab_titles = vec!["[OVERVIEW]", "[RESOURCES]", "[NETWORK]", "[LOGS]", "[SETTINGS]"];

    let tabs = Tabs::new(tab_titles)
        .select(match app.current_tab {
            Tab::Overview => 0,
            Tab::Resources => 1,
            Tab::Network => 2,
            Tab::Logs => 3,
            Tab::Settings => 4,
        })
        .style(Style::default().fg(Color::Green))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" NAVIGATION MODULE ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

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
        format!("DAEMON STATUS: {} | PID: {} | PROCESS: {}", daemon_state, pid, app.daemon_manager.get_process_name())
    } else {
        format!("DAEMON STATUS: {} | SEARCHING FOR: {}", daemon_state, app.daemon_manager.get_process_name())
    };
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(state_color).add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" SYSTEM STATUS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(status, chunks[0]);

    // Key metrics
    let stats = app.daemon_manager.get_process_stats();
    let conn_status = app.network_monitor.get_connection_status();
    let net_stats = app.network_monitor.get_network_stats();

    let metrics_text = vec![
        Line::from(vec![
            Span::styled("CPU USAGE: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:.1}%", stats.cpu_usage), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("MEMORY: ", Style::default().fg(Color::Green)),
            Span::styled(format_memory(stats.memory_usage), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("CONNECTION: ", Style::default().fg(Color::Green)),
            Span::styled(
                if conn_status.is_connected { "CONNECTED" } else { "DISCONNECTED" },
                Style::default().fg(if conn_status.is_connected { Color::Green } else { Color::Red }).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("DATA FLOW: ", Style::default().fg(Color::Green)),
            Span::styled(
                if net_stats.data_flow_active { "ACTIVE" } else { "INACTIVE" },
                Style::default().fg(if net_stats.data_flow_active { Color::Green } else { Color::Red }).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let metrics = Paragraph::new(metrics_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" CORE METRICS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

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
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" SERVICE LOGS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

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
    let cpu_load_text = format!("PROCESSOR: {:.1}% | SYSTEM LOAD: {:.2}",
        stats.cpu_usage,
        stats.system_load_avg
    );
    let cpu_load = Paragraph::new(cpu_load_text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" CPU CORE ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(cpu_load, chunks[0]);

    // Memory usage (Process and System)
    let memory_text = format!("PROCESS: {} | VIRTUAL: {}\nSYSTEM: {} / {}",
        format_memory(stats.memory_usage),
        format_memory(stats.virtual_memory),
        format_memory(stats.system_memory_used),
        format_memory(stats.system_memory_total)
    );
    let memory = Paragraph::new(memory_text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" MEMORY MODULE ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(memory, chunks[1]);

    // I/O Statistics
    let io_text = format!("DISK READ: {} | WRITE: {}\nNET RX: {} | TX: {}",
        format_bytes(stats.disk_read_bytes),
        format_bytes(stats.disk_write_bytes),
        format_bytes(stats.network_rx_bytes),
        format_bytes(stats.network_tx_bytes)
    );
    let io_stats = Paragraph::new(io_text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" I/O SYSTEMS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(io_stats, chunks[2]);

    // Process Details
    let details_text = format!("PROCESS: {}\nPID: {:?} | PPID: {:?}\nSTATE: {} | PRIORITY: {}\nTHREADS: {} | OPEN FILES: {}\nUPTIME: {}s",
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
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" PROCESS INFO ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(details, chunks[3]);

    // Additional Info
    let additional_text = format!("START TIME: {}\nCONTEXT SWITCHES: {}\nPAGE FAULTS: {}",
        format_timestamp(stats.start_time),
        stats.context_switches,
        stats.page_faults
    );
    let additional = Paragraph::new(additional_text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" ADVANCED METRICS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

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
    let conn_text = format!("STATUS: {}\nENDPOINT: {}\nDURATION: {:.1}s",
        if conn_status.is_connected { "CONNECTED" } else { "DISCONNECTED" },
        conn_status.endpoint,
        conn_status.connection_duration.as_secs_f32()
    );
    let connection = Paragraph::new(conn_text)
        .style(Style::default().fg(conn_color))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" CONNECTION STATUS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(connection, chunks[0]);

    // Traffic stats
    let traffic_text = format!("SENT: {} ({} PACKETS)\nRECEIVED: {} ({} PACKETS)\nDATA FLOW: {}",
        format_bytes(net_stats.bytes_sent),
        net_stats.packets_sent,
        format_bytes(net_stats.bytes_received),
        net_stats.packets_received,
        if net_stats.data_flow_active { "ACTIVE" } else { "INACTIVE" }
    );
    let traffic = Paragraph::new(traffic_text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" NETWORK TRAFFIC ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(traffic, chunks[1]);

    // Additional details
    let details = Paragraph::new("NETWORK MONITORING DETAILS WILL BE DISPLAYED HERE...")
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" NETWORK ANALYSIS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(details, chunks[2]);
}

fn draw_logs(f: &mut Frame, area: Rect, _app: &App) {
    let logs = Paragraph::new("LOG VIEWER WILL BE IMPLEMENTED HERE...\n\nUSE ARROW KEYS TO SCROLL\nPRESS 'F' TO FILTER LOGS")
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true })
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" SYSTEM LOGS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(logs, area);
}

fn draw_settings(f: &mut Frame, area: Rect, _app: &App) {
    let settings = Paragraph::new("SETTINGS WILL BE IMPLEMENTED HERE...\n\n- REFRESH RATE\n- ALERT THRESHOLDS\n- DAEMON CONFIGURATION\n- NETWORK ENDPOINTS")
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" CONFIGURATION ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(settings, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let management_type = if app.daemon_manager.is_using_systemctl() {
        "SYSTEMCTL"
    } else {
        "DIRECT"
    };

    let status_text = format!(
        " [F1-F5] NAV | [H/L/J/K] MOVE | [S] START | [X] STOP | [R] REFRESH | [Q] QUIT | MODE: {} | INTERVAL: {}ms ",
        management_type,
        app.refresh_rate.as_millis()
    );

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Green).bg(Color::Black))
        .alignment(Alignment::Center)
        .block(Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Green))
            .title(" STATUS MODULE ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

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