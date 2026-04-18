//! cozby-tui — terminal UI for cozby-brain using ratatui.
//!
//! Tabs: Inbox | Notes | Todos | Reminders | Learning
//! - Inbox: chat-like field, enter → POST /api/ingest → LLM routes to correct type
//! - Lists: browse existing entities
//!
//! Indicators (no emoji):
//!   ●  filled / active / strong
//!   ○  empty / wiki-link / weak
//!   ■  important / delivered
//!   □  inactive / pending
//!   ✓  done / learned
//!   ✗  skipped / error
//!
//! Color correlation:
//!   green  — strong / success / done
//!   yellow — medium / pending-due
//!   red    — overdue / error
//!   cyan   — link / info
//!   gray   — weak / inactive

mod app;
mod indicators;
mod views;

use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{App, Tab};

#[derive(Parser)]
#[command(name = "cozby-tui", about = "Terminal UI for cozby-brain")]
struct Cli {
    #[arg(long, env = "COZBY_API", default_value = "http://localhost:8081")]
    api: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(cli.api);
    app.refresh_current_tab().await;

    let res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("error: {e}");
    }
    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| views::render(f, app))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') if !app.input_mode => break,
                KeyCode::Esc if app.input_mode => {
                    app.input_mode = false;
                    app.status = String::new();
                }
                KeyCode::Tab if !app.input_mode => {
                    app.next_tab();
                    app.refresh_current_tab().await;
                }
                KeyCode::BackTab if !app.input_mode => {
                    app.prev_tab();
                    app.refresh_current_tab().await;
                }
                KeyCode::Char('i') if !app.input_mode => {
                    app.tab = Tab::Inbox;
                    app.input_mode = true;
                    app.input.clear();
                }
                KeyCode::Char('r') if !app.input_mode => {
                    app.refresh_current_tab().await;
                }
                KeyCode::Enter if app.input_mode && matches!(app.tab, Tab::Inbox) => {
                    let text = app.input.trim().to_string();
                    if !text.is_empty() {
                        app.input.clear();
                        app.input_mode = false;
                        app.status = "отправляю в /api/ingest…".into();
                        terminal.draw(|f| views::render(f, app))?;
                        app.send_ingest(&text).await;
                    } else {
                        app.input_mode = false;
                    }
                }
                KeyCode::Char(c) if app.input_mode => {
                    app.input.push(c);
                }
                KeyCode::Backspace if app.input_mode => {
                    app.input.pop();
                }
                KeyCode::Down if !app.input_mode => {
                    app.next_item();
                }
                KeyCode::Up if !app.input_mode => {
                    app.prev_item();
                }
                _ => {}
            }
        }
    }
    Ok(())
}
