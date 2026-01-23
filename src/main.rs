use std::io::{self, stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{EnableMouseCapture, DisableMouseCapture},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use libc;

mod app;
mod daemon;
mod error;
mod network;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Show startup animation
    ui::show_startup_animation(&mut terminal).await?;

    // Check if running as root after animation
    let is_root = unsafe { libc::geteuid() == 0 };
    if !is_root {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        std::process::exit(1);
    }

    // Create app and run it
    let app = App::new()?;
    let res = run_app(&mut terminal, app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    mut app: App,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(100); // Keep original tick rate for data updates
    let mut last_draw = Instant::now();
    let draw_rate = Duration::from_millis(16); // 60 FPS drawing

    loop {
        // Draw at 60 FPS for smooth UI
        if last_draw.elapsed() >= draw_rate {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            last_draw = Instant::now();
        }

        // Poll for events with very short timeout for responsive input
        if crossterm::event::poll(Duration::from_millis(1))? {
            match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => {
                    app.on_key(key);
                }
                crossterm::event::Event::Mouse(mouse) => {
                    app.on_mouse(mouse);
                }
                _ => {}
            }
        }

        // Handle data updates at original rate
        if last_tick.elapsed() >= tick_rate {
            app.on_tick().await;
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }

        // Small sleep to prevent 100% CPU usage
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}