//! cozby-tui — event-driven terminal UI.
//!
//! Главный цикл: читает terminal-events + app-events параллельно.
//! Сеть/LLM-вызовы идут в фоне через [`worker`], UI не блокируется.
//!
//! Клавиши (Normal mode):
//!   h/l, ←/→, Tab    переключение вкладок
//!   j/k, ↓/↑          навигация по списку
//!   g / G             первый / последний элемент
//!   i                 ingest-режим (chat-field)
//!   /                 search-режим (фильтр списка)
//!   r                 refresh текущей вкладки
//!   q / Esc           выход
//!
//! В Ingest / Search: Enter → submit, Esc → отмена.

mod app;
mod indicators;
mod theme;
mod views;

use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::{spawn_worker, App, AppCmd, AppEvent, Mode, Tab};

#[derive(Parser)]
#[command(name = "cozby-tui", about = "Terminal UI for cozby-brain")]
struct Cli {
    #[arg(long, env = "COZBY_API", default_value = "http://localhost:8081")]
    api: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Channels
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<AppCmd>();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AppEvent>();

    // Worker (API client)
    let api_for_worker = cli.api.clone();
    tokio::spawn(async move {
        spawn_worker(api_for_worker, cmd_rx, ev_tx).await;
    });

    let mut app = App::new(cli.api.clone(), cmd_tx.clone());
    // initial ping + default tab
    let _ = cmd_tx.send(AppCmd::Ping);
    app.switch_tab(Tab::Notes); // triggers refresh

    let res = run(&mut terminal, &mut app, &mut ev_rx).await;

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

async fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    ev_rx: &mut mpsc::UnboundedReceiver<AppEvent>,
) -> Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_period = Duration::from_millis(90);

    loop {
        terminal.draw(|f| views::render(f, app))?;

        // Poll terminal + channel concurrently.
        // crossterm::event::poll is blocking, so wrap in spawn_blocking? No —
        // just use short poll timeout and check channel afterwards.
        let timeout = tick_period
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_millis(10));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if !handle_key(app, key.code, key.modifiers) {
                    break;
                }
            }
        }

        // Drain app events (non-blocking).
        while let Ok(ev) = ev_rx.try_recv() {
            apply_event(app, ev);
        }

        if last_tick.elapsed() >= tick_period {
            if app.loading {
                app.tick();
            }
            last_tick = std::time::Instant::now();
        }
    }
    Ok(())
}

fn apply_event(app: &mut App, ev: AppEvent) {
    match ev {
        AppEvent::Loaded(tab, items) => {
            if tab == app.tab {
                app.items = items;
                app.selected = 0;
                app.loading = false;
                app.status = format!("{} · loaded {}", tab.label(), app.items.len());
            }
        }
        AppEvent::Ingested(v) => {
            app.last_ingest = Some(v.clone());
            app.loading = false;
            // Описать что пришло
            let label = if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                format!("classified {} items", items.len())
            } else if let Some(kind) = v.get("type").and_then(|x| x.as_str()) {
                format!("{kind} created")
            } else {
                "ingested".into()
            };
            app.status = label;
            // refresh current tab в фоне
            app.refresh();
        }
        AppEvent::Pong(ok) => {
            app.connected = ok;
            if !ok {
                app.status = "сервер не отвечает".into();
            }
        }
        AppEvent::Error(e) => {
            app.loading = false;
            app.status = format!("ошибка: {e}");
        }
        AppEvent::Tick => {
            app.tick();
        }
    }
}

/// Returns false if app should exit.
fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> bool {
    // Ctrl+C — всегда выход
    if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        return false;
    }

    match app.mode {
        Mode::Normal => handle_normal(app, code),
        Mode::Ingest => handle_ingest(app, code),
        Mode::Search => handle_search(app, code),
    }
}

fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => app.next_tab(),
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => app.prev_tab(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
        KeyCode::Char('g') => app.select_first(),
        KeyCode::Char('G') => app.select_last(),
        KeyCode::Char('r') => app.refresh(),
        KeyCode::Char('i') => {
            app.tab = Tab::Inbox;
            app.mode = Mode::Ingest;
            app.input.clear();
        }
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.search.clear();
        }
        // Быстрые переходы на вкладки по первой букве
        KeyCode::Char('1') => app.switch_tab(Tab::Inbox),
        KeyCode::Char('2') => app.switch_tab(Tab::Notes),
        KeyCode::Char('3') => app.switch_tab(Tab::Todos),
        KeyCode::Char('4') => app.switch_tab(Tab::Reminders),
        KeyCode::Char('5') => app.switch_tab(Tab::Learning),
        KeyCode::Char('6') => app.switch_tab(Tab::Docs),
        _ => {}
    }
    true
}

fn handle_ingest(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input.clear();
        }
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            let text = text.trim().to_string();
            app.mode = Mode::Normal;
            if !text.is_empty() {
                app.loading = true;
                app.status = "LLM обрабатывает…".into();
                let _ = app.cmd_tx.send(AppCmd::Ingest(text));
            }
        }
        KeyCode::Char(c) => app.input.push(c),
        KeyCode::Backspace => {
            app.input.pop();
        }
        _ => {}
    }
    true
}

fn handle_search(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.search.clear();
            app.selected = 0;
        }
        KeyCode::Enter => {
            app.mode = Mode::Normal;
            app.selected = 0;
        }
        KeyCode::Char(c) => {
            app.search.push(c);
            app.selected = 0;
        }
        KeyCode::Backspace => {
            app.search.pop();
            app.selected = 0;
        }
        _ => {}
    }
    true
}
