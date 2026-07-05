use std::io::{self, stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod cli;
mod daemon;
mod network;
mod paths;
mod ui;

use app::App;

#[derive(Parser)]
#[command(
    name = "lsman",
    version,
    about = "Debug and manage the LSI (SysTrack) agent",
    long_about = "Debug and manage the LSI (SysTrack) agent.\n\n\
                  Run without a subcommand for the interactive TUI. Subcommands give\n\
                  script-friendly access to daemon status, logs, trace levels, the\n\
                  agent database, and crash reports."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<cli::CliCommand>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Cli::parse();
    match args.command {
        Some(command) => cli::run(command).await,
        None => run_tui().await,
    }
}

async fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    ui::show_startup_animation(&mut terminal).await?;

    let app = App::new()?;
    let res = run_app(&mut terminal, app).await;

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
