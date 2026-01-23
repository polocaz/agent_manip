use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Frame,
    Terminal, backend::CrosstermBackend,
};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};

use crate::app::{App, Tab};
use std::path::PathBuf;

/// Check for keyboard input during animation
/// Returns Some(KeyCode) if a key was pressed, None otherwise
fn check_animation_input() -> Option<KeyCode> {
    if event::poll(std::time::Duration::from_millis(10)).unwrap_or(false) {
        if let Ok(Event::Key(key)) = event::read() {
            return Some(key.code);
        }
    }
    None
}

/// Get cached log content, updating cache if file has changed
/// Limits content to last MAX_LOG_LINES for performance
fn get_cached_log_content(log_path: &std::path::Path, log_index: usize, cache: &mut std::collections::HashMap<usize, crate::app::CachedLog>) -> (String, bool) {
    const MAX_LOG_LINES: usize = 1000; // Limit to last 1000 lines for performance
    
    // Check if we have cached content
    if let Some(cached) = cache.get(&log_index) {
        // Check if file still exists and hasn't been modified
        if let Ok(metadata) = std::fs::metadata(log_path) {
            if let Ok(modified) = metadata.modified() {
                // If file hasn't changed, use cached content
                if modified == cached.modified {
                    return (cached.content.clone(), false); // false = no change
                }
            }
        }
    }
    
    // File doesn't exist in cache or has been modified, read it
    let full_content = match std::fs::read_to_string(log_path) {
        Ok(content) => content,
        Err(e) => {
            return (format!("UNABLE TO READ LOG FILE\n\nPATH: {}\n\nERROR: {}\n\nPossible causes:\n• File does not exist\n• Insufficient permissions\n• Daemon not installed\n\nTry running with elevated privileges or check installation.", log_path.display(), e), true);
        }
    };
    
    // Process content: limit to last MAX_LOG_LINES and handle empty files
    let content = if full_content.is_empty() {
        format!("LOG FILE IS EMPTY\n\nPATH: {}\n\nThe log file exists but contains no content.", log_path.display())
    } else {
        // Split into lines and take the last MAX_LOG_LINES
        let lines: Vec<&str> = full_content.lines().collect();
        let start_idx = if lines.len() > MAX_LOG_LINES {
            lines.len() - MAX_LOG_LINES
        } else {
            0
        };
        
        let limited_lines = &lines[start_idx..];
        let limited_content = limited_lines.join("\n");
        
        // Add a note if content was truncated
        if lines.len() > MAX_LOG_LINES {
            format!("... (showing last {} of {} lines)\n\n{}", MAX_LOG_LINES, lines.len(), limited_content)
        } else {
            limited_content
        }
    };
    let displayed_line_count = content.lines().count();
    
    let modified = std::fs::metadata(log_path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    
    cache.insert(log_index, crate::app::CachedLog {
        content: content.clone(),
        modified,
        line_count: displayed_line_count, // Use displayed line count for scrolling
    });
    
    (content, true) // true = content changed
}

/// Get the log file path based on platform and log file index
fn get_log_file_path(log_index: usize) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from(format!("/var/opt/lsiagent/lsiagent{}.log", log_index))
    }

    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/Lakeside Software/lsiagent.log")
    }

    #[cfg(target_os = "windows")]
    {
        PathBuf::from(format!("C:\\Program Files\\Lakeside Software\\LsiAgent{}.log", log_index))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // Fallback for other platforms
        PathBuf::from(format!("/var/log/lsiagent{}.log", log_index))
    }
}

/// Check which log files exist on the system
pub fn get_available_log_files() -> Vec<usize> {
    let mut available = Vec::new();
    
    for i in 0..10 {
        let path = get_log_file_path(i);
        if path.exists() {
            available.push(i);
        }
    }
    
    available
}

pub async fn show_startup_animation(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    // Clear the screen first
    terminal.clear()?;

    // Check if running as root
    let is_root = unsafe { libc::geteuid() == 0 };

    // Define the final complete frames
    let final_frames = vec![
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
        // Frame 4: Status (different based on permissions)
        if is_root {
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
            ]
        } else {
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
                Line::from(vec![Span::styled("> STATUS: INOPERATIONAL", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))]),
                Line::from(vec![]),
                Line::from(vec![Span::styled("> ERROR: INSUFFICIENT PERMISSIONS", Style::default().fg(Color::Red))]),
                Line::from(vec![Span::styled("> TRY LAUNCHING AS ADMIN", Style::default().fg(Color::Red))]),
            ]
        },
    ];

    // Typing animation for each frame
    for (frame_index, final_frame) in final_frames.iter().enumerate() {
        let mut current_frame = vec![];

        // Copy existing lines from previous frames
        if frame_index > 0 {
            current_frame.extend_from_slice(&final_frames[frame_index - 1]);
        }

        // Find new lines to type
        let start_line = if frame_index == 0 { 0 } else { final_frames[frame_index - 1].len() };
        let new_lines = &final_frame[start_line..];

        // Type out new lines - different behavior for ASCII art vs text
        for (line_idx, line) in new_lines.iter().enumerate() {
            let actual_line_idx = start_line + line_idx;

            // Skip empty lines - show them instantly
            if line.spans.is_empty() || (line.spans.len() == 1 && line.spans[0].content.is_empty()) {
                current_frame.push(line.clone());
                continue;
            }

            // Special handling for ASCII art frame (frame_index == 2) - show complete lines
            if frame_index == 2 {
                // For ASCII art, show each line completely at once
                current_frame.resize(actual_line_idx, Line::from(vec![]));
                if actual_line_idx < current_frame.len() {
                    current_frame[actual_line_idx] = line.clone();
                } else {
                    current_frame.push(line.clone());
                }

                // Draw the current state with the complete line
                terminal.draw(|f| {
                    let size = f.size();
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Min(10), // Main animation area
                            Constraint::Length(1), // Status line
                        ])
                        .split(size);

                    // Main animation content
                    let paragraph = Paragraph::new(current_frame.clone())
                        .alignment(Alignment::Center)
                        .block(Block::default().borders(Borders::NONE));
                    f.render_widget(paragraph, chunks[0]);

                    // Status line with controls
                    let status = Paragraph::new(Line::from(vec![
                        Span::styled("[ENTER]", Style::default().fg(Color::Cyan)),
                        Span::raw(" Skip "),
                        Span::styled("[Q]", Style::default().fg(Color::Cyan)),
                        Span::raw(" Quit"),
                    ]))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::White));
                    f.render_widget(status, chunks[1]);
                })?;

                // Small delay between lines for cascading effect
                // Check for user input to skip or quit
                if let Some(key) = check_animation_input() {
                    match key {
                        KeyCode::Enter => {
                            // Skip to final frame
                            terminal.draw(|f| {
                                let size = f.size();
                                let chunks = Layout::default()
                                    .direction(Direction::Vertical)
                                    .constraints([
                                        Constraint::Min(10), // Main animation area
                                        Constraint::Length(1), // Status line
                                    ])
                                    .split(size);

                                // Main animation content
                                let paragraph = Paragraph::new(final_frames.last().unwrap().clone())
                                    .alignment(Alignment::Center)
                                    .block(Block::default().borders(Borders::NONE));
                                f.render_widget(paragraph, chunks[0]);

                                // Status line with controls
                                let status = Paragraph::new(Line::from(vec![
                                    Span::styled("[ENTER]", Style::default().fg(Color::Cyan)),
                                    Span::raw(" Skip Animation  "),
                                    Span::styled("[Q]", Style::default().fg(Color::Cyan)),
                                    Span::raw(" Quit"),
                                ]))
                                .alignment(Alignment::Center)
                                .style(Style::default().fg(Color::White));
                                f.render_widget(status, chunks[1]);
                            })?;
                            return Ok(());
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            // Exit application
                            std::process::exit(0);
                        }
                        _ => {}
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
            } else {
                // For regular text, type character by character
                if let Some(span) = line.spans.first() {
                    let text = span.content.as_ref();
                    let chars: Vec<char> = text.chars().collect();

                    for char_idx in 0..=chars.len() {
                        // Build the current line with partial text
                        let partial_text: String = chars.iter().take(char_idx).collect();
                        let mut partial_line = line.clone();
                        if let Some(span_mut) = partial_line.spans.first_mut() {
                            span_mut.content = partial_text.into();
                        }

                        // Update the current frame
                        current_frame.resize(actual_line_idx, Line::from(vec![]));
                        if actual_line_idx < current_frame.len() {
                            current_frame[actual_line_idx] = partial_line;
                        } else {
                            current_frame.push(partial_line);
                        }

                        // Draw the current state
                        terminal.draw(|f| {
                            let size = f.size();
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Min(10), // Main animation area
                                    Constraint::Length(1), // Status line
                                ])
                                .split(size);

                            // Main animation content
                            let paragraph = Paragraph::new(current_frame.clone())
                                .alignment(Alignment::Center)
                                .block(Block::default().borders(Borders::NONE));
                            f.render_widget(paragraph, chunks[0]);

                            // Status line with controls
                            let status = Paragraph::new(Line::from(vec![
                                Span::styled("[ENTER]", Style::default().fg(Color::Cyan)),
                                Span::raw(" Skip Animation  "),
                                Span::styled("[Q]", Style::default().fg(Color::Cyan)),
                                Span::raw(" Quit"),
                            ]))
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::White));
                            f.render_widget(status, chunks[1]);
                        })?;

                        // Typing delay (ultra fast for instant boot)
                        // Check for user input to skip or quit
                        if let Some(key) = check_animation_input() {
                            match key {
                                KeyCode::Enter => {
                                    // Skip to final frame
                                    terminal.draw(|f| {
                                        let size = f.size();
                                        let chunks = Layout::default()
                                            .direction(Direction::Vertical)
                                            .constraints([
                                                Constraint::Min(10), // Main animation area
                                                Constraint::Length(1), // Status line
                                            ])
                                            .split(size);

                                        // Main animation content
                                        let paragraph = Paragraph::new(final_frames.last().unwrap().clone())
                                            .alignment(Alignment::Center)
                                            .block(Block::default().borders(Borders::NONE));
                                        f.render_widget(paragraph, chunks[0]);

                                        // Status line with controls
                                        let status = Paragraph::new(Line::from(vec![
                                            Span::styled("[ENTER]", Style::default().fg(Color::Cyan)),
                                            Span::raw(" Skip Animation  "),
                                            Span::styled("[Q]", Style::default().fg(Color::Cyan)),
                                            Span::raw(" Quit"),
                                        ]))
                                        .alignment(Alignment::Center)
                                        .style(Style::default().fg(Color::White));
                                        f.render_widget(status, chunks[1]);
                                    })?;
                                    return Ok(());
                                }
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    // Exit application
                                    std::process::exit(0);
                                }
                                _ => {}
                            }
                        }
                        let char_delay = 0; // No delay - instant appearance
                        tokio::time::sleep(tokio::time::Duration::from_millis(char_delay)).await;
                    }
                }
            }
        }

        // Pause after each complete frame (ultra fast boot)
        let frame_delay = match frame_index {
            0 => 0, // Header - instant
            1 => 0, // Loading message - instant
            2 => 0, // ASCII art - instant
            3 => if is_root { 500 } else { 0 }, // Final status - brief pause for success, instant for error
            _ => 0,
        };

        // Check for input during frame delays
        if frame_delay > 0 {
            if let Some(key) = check_animation_input() {
                match key {
                    KeyCode::Enter => {
                        // Skip to final frame
                        terminal.draw(|f| {
                            let size = f.size();
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Min(10), // Main animation area
                                    Constraint::Length(1), // Status line
                                ])
                                .split(size);

                            // Main animation content
                            let paragraph = Paragraph::new(final_frames.last().unwrap().clone())
                                .alignment(Alignment::Center)
                                .block(Block::default().borders(Borders::NONE));
                            f.render_widget(paragraph, chunks[0]);

                            // Status line with controls
                            let status = Paragraph::new(Line::from(vec![
                                Span::styled("[ENTER]", Style::default().fg(Color::Cyan)),
                                Span::raw(" Skip Animation  "),
                                Span::styled("[Q]", Style::default().fg(Color::Cyan)),
                                Span::raw(" Quit"),
                            ]))
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::White));
                            f.render_widget(status, chunks[1]);
                        })?;
                        return Ok(());
                    }
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        // Exit application
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(frame_delay)).await;
    }

    // Final pause with all content visible
    if is_root {
        // Success case: pause 1 second with all content visible
        // Check for input during final pause
        if let Some(key) = check_animation_input() {
            match key {
                KeyCode::Enter => {
                    // Skip final pause
                    return Ok(());
                }
                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    // Exit application
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    } else {
        // Error case: add "PRESS ENTER TO EXIT" prompt and wait for input
        // First, update the display to show the prompt
        let mut error_frame = final_frames.last().unwrap().clone();
        error_frame.push(Line::from(vec![]));
        error_frame.push(Line::from(vec![Span::styled("> PRESS ENTER TO EXIT", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]));

        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(10), // Main animation area
                    Constraint::Length(1), // Status line
                ])
                .split(size);

            // Main animation content
            let paragraph = Paragraph::new(error_frame)
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::NONE));
            f.render_widget(paragraph, chunks[0]);

            // Status line with controls
            let status = Paragraph::new(Line::from(vec![
                Span::styled("[ENTER]", Style::default().fg(Color::Cyan)),
                Span::raw(" Skip Animation  "),
                Span::styled("[Q]", Style::default().fg(Color::Cyan)),
                Span::raw(" Quit"),
            ]))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White));
            f.render_widget(status, chunks[1]);
        })?;

        // Wait for user to press Enter (we'll handle this in main.rs)
        // For now, just pause for 5 seconds as requested
        tokio::time::sleep(tokio::time::Duration::from_millis(5000)).await;
    }

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

fn draw_tabs(f: &mut Frame, area: Rect, app: &mut App) {
    let tab_titles = vec!["[OVERVIEW]", "[RESOURCES]", "[NETWORK]", "[LOGS]", "[CONFIG]", "[SETTINGS]"];

    let tabs = Tabs::new(tab_titles)
        .select(match app.current_tab {
            Tab::Overview => 0,
            Tab::Resources => 1,
            Tab::Network => 2,
            Tab::Logs => 3,
            Tab::Config => 4,
            Tab::Settings => 5,
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

fn draw_main_content(f: &mut Frame, area: Rect, app: &mut App) {
    match app.current_tab {
        Tab::Overview => draw_overview(f, area, app),
        Tab::Resources => draw_resources(f, area, app),
        Tab::Network => draw_network(f, area, app),
        Tab::Logs => draw_logs(f, area, app),
        Tab::Config => draw_config(f, area, app),
        Tab::Settings => draw_settings(f, area, app),
    }
}

fn draw_overview(f: &mut Frame, area: Rect, app: &mut App) {
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

fn draw_resources(f: &mut Frame, area: Rect, app: &mut App) {
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

fn draw_network(f: &mut Frame, area: Rect, app: &mut App) {
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

fn draw_logs(f: &mut Frame, area: Rect, app: &mut App) {
    let log_path = get_log_file_path(app.current_log_file);
    
    // Get or update cached log content
    let (log_content, content_changed) = get_cached_log_content(&log_path, app.current_log_file, &mut app.log_cache);
    
    // Check if log content has grown (new logs added) for auto-scroll
    let current_content_len = log_content.len();
    if content_changed && current_content_len > app.last_log_content_len && app.current_tab == Tab::Logs {
        // Content has grown, auto-scroll to bottom
        // Calculate visible height (area height minus borders)
        let visible_height = (area.height as usize).saturating_sub(2); // Subtract borders
        
        // Get line count from cache if available
        let content_lines = if let Some(cached) = app.log_cache.get(&app.current_log_file) {
            cached.line_count
        } else {
            log_content.lines().count()
        };
        
        // Set scroll to show the bottom of the content
        if content_lines > visible_height {
            app.logs_scroll = (content_lines - visible_height) as u16;
        } else {
            app.logs_scroll = 0;
        }
    }
    
    // Update the last known content length only if content changed
    if content_changed {
        app.last_log_content_len = current_content_len;
    }

    // Clamp scroll position to valid bounds
    let visible_height = (area.height as usize).saturating_sub(2);
    let content_lines = if let Some(cached) = app.log_cache.get(&app.current_log_file) {
        cached.line_count
    } else {
        log_content.lines().count()
    };
    
    let max_scroll = if content_lines > visible_height {
        content_lines - visible_height
    } else {
        0
    };
    
    if app.logs_scroll > max_scroll as u16 {
        app.logs_scroll = max_scroll as u16;
    }

    // Create log file tabs for available files
    let available_logs = get_available_log_files();

    // Ensure currently selected log is valid. If not, pick the first available.
    if !available_logs.is_empty() && !available_logs.contains(&app.current_log_file) {
        app.current_log_file = available_logs[0];
        app.logs_scroll = 0;
        app.last_log_content_len = 0;
    }

    let mut log_tabs = Vec::new();

    // If no logs found, add a placeholder
    if available_logs.is_empty() {
        log_tabs.push(Span::styled("[NO LOGS FOUND]", Style::default().fg(Color::Red)));
    } else {
        for &log_idx in &available_logs {
        let tab_text = if log_idx == app.current_log_file {
            format!("[{}*]", log_idx)
        } else {
            format!("[{}]", log_idx)
        };
        
        let style = if log_idx == app.current_log_file {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else {
            Style::default().fg(Color::Green)
        };
        
        log_tabs.push(Span::styled(tab_text, style));
        log_tabs.push(Span::raw(" "));
    }
    }

    // Split the area for log tabs and content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Log file tabs
            Constraint::Min(1),    // Log content
        ])
        .split(area);

    // Draw log file tabs with a compact label and clear highlight for selection.
    // Build a single-line composed of a left label followed by the tab spans.
    let mut composed_spans = Vec::new();

    // Left label
    composed_spans.push(Span::styled("LOGS: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    // Append the tab spans we already built
    composed_spans.extend(log_tabs.into_iter());

    // Convert to a single Line and render inside a Paragraph without title
    let tabs_line = Line::from(composed_spans);
    let tabs_paragraph = Paragraph::new(vec![tabs_line])
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::Green)))
        .style(Style::default().fg(Color::Green));

    f.render_widget(tabs_paragraph, chunks[0]);

    // Draw log content
    let title = format!(" LOG FILE {}: {} ", app.current_log_file, log_path.file_name().unwrap_or_default().to_string_lossy());

    let logs = Paragraph::new(log_content)
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true })
        .scroll((app.logs_scroll, 0))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(title)
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(logs, chunks[1]);
}

fn draw_config(f: &mut Frame, area: Rect, app: &mut App) {
    let config_content = match std::fs::read_to_string("/var/opt/lsiagent/lsiagent.cfg") {
        Ok(content) => content,
        Err(_) => "CONFIG FILE NOT FOUND OR ACCESS DENIED\n\nPATH: /var/opt/lsiagent/lsiagent.cfg\n\nThis file contains LSI Agent configuration settings.\nEnsure the daemon is properly installed and you have read permissions.\n\nYou can also check the example config file at: ./example/lsiagent.cfg".to_string(),
    };

    let config = Paragraph::new(config_content)
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true })
        .scroll((app.config_scroll, 0))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" AGENT CONFIGURATION ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(config, area);
}

fn draw_settings(f: &mut Frame, area: Rect, _app: &mut App) {
    let settings = Paragraph::new("SETTINGS WILL BE IMPLEMENTED HERE...\n\n- REFRESH RATE\n- ALERT THRESHOLDS\n- DAEMON CONFIGURATION\n- NETWORK ENDPOINTS")
        .style(Style::default().fg(Color::Green))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" CONFIGURATION ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    f.render_widget(settings, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &mut App) {
    let management_type = if app.daemon_manager.is_using_systemctl() {
        "SYSTEMCTL"
    } else {
        "DIRECT"
    };

    let base_status = format!(
        " [F1-F6] NAV | [H/L/J/K] MOVE | [S] START | [X] STOP | [R] REFRESH | [Q] QUIT | [MOUSE] CLICK TABS | MODE: {} | INTERVAL: {}ms ",
        management_type,
        app.refresh_rate.as_millis()
    );

    let status_text = match app.current_tab {
        Tab::Config => format!("{} | [↑/↓] SCROLL | [PgUp/PgDn] PAGE | [MOUSE] WHEEL ", base_status),
        Tab::Logs => format!("{} | [↑/↓] SCROLL | [PgUp/PgDn] PAGE | [MOUSE] WHEEL+TABS | [←/→] LOG FILE | [0-9] SELECT FILE ", base_status),
        _ => base_status,
    };

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