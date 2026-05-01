//! cozby-tui — event-driven terminal UI.
//!
//! Управление (Normal mode):
//!   Tab / Shift+Tab      цикл фокуса (sidebar → list → detail)
//!   ]t / [t  или  1-6    смена вкладок
//!   j / k   ↓ / ↑         навигация
//!   g / G                 первый / последний
//!   Enter  /  o           открыть запись (detail или раскрыть проект в Docs)
//!   Space                 toggle done (для todo)
//!   d                     удалить (с подтверждением y/n)
//!   i                     ingest
//!   /                     search-фильтр
//!   :                     командный режим (:notes, :docs, :all, :recent, :q, …)
//!   r                     refresh
//!   Esc                   закрыть detail / отменить ввод
//!   q                     выход (только если ничего не открыто)

mod app;
mod indicators;
mod markdown;
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

use crate::app::{spawn_worker, App, AppCmd, AppEvent, Focus, Mode, Tab};

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

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<AppCmd>();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AppEvent>();

    let api_for_worker = cli.api.clone();
    tokio::spawn(async move {
        spawn_worker(api_for_worker, cmd_rx, ev_tx).await;
    });

    let mut app = App::new(cli.api.clone(), cmd_tx.clone());
    let _ = cmd_tx.send(AppCmd::Ping);
    app.switch_tab(Tab::Notes);

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
    let mut pending_tab_key = false; // for `]t` / `[t`

    loop {
        terminal.draw(|f| views::render(f, app))?;

        let timeout = tick_period
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_millis(10));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if !handle_key(app, key.code, key.modifiers, &mut pending_tab_key) {
                    break;
                }
            }
        }

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
                app.status = format!("{} · {} items", tab.label(), app.items.len());
            }
        }
        AppEvent::PagesLoaded(project_id, pages) => {
            app.doc_pages.insert(project_id, pages);
            app.loading = false;
        }
        AppEvent::NoteDetail(v) => {
            app.opened = Some(v);
            app.focus = Focus::Detail;
            app.loading = false;
            app.detail_scroll = 0;
        }
        AppEvent::PageDetail(v) => {
            app.opened = Some(v);
            app.focus = Focus::Detail;
            app.loading = false;
            app.detail_scroll = 0;
        }
        AppEvent::Ingested(v) => {
            app.last_ingest = Some(v.clone());
            app.loading = false;
            let label = if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                format!("classified {} items", items.len())
            } else if let Some(kind) = v.get("type").and_then(|x| x.as_str()) {
                format!("{kind} saved")
            } else {
                "ingested".into()
            };
            app.status = label;
            app.refresh();
        }
        AppEvent::Deleted(_, _) => {
            app.status = "deleted".into();
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
        AppEvent::Tick => app.tick(),
    }
}

/// Returns false to exit.
fn handle_key(
    app: &mut App,
    code: KeyCode,
    mods: KeyModifiers,
    pending_tab_key: &mut bool,
) -> bool {
    if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        return false;
    }

    match app.mode {
        Mode::Normal => handle_normal(app, code, pending_tab_key),
        Mode::Ingest => handle_ingest(app, code, mods),
        Mode::Search => handle_search(app, code),
        Mode::Command => handle_command(app, code),
        Mode::Confirm => handle_confirm(app, code),
    }
}

fn handle_normal(app: &mut App, code: KeyCode, pending_tab_key: &mut bool) -> bool {
    // `]t` / `[t` — next/prev tab (pending-key state)
    if *pending_tab_key {
        *pending_tab_key = false;
        if matches!(code, KeyCode::Char('t')) {
            // just consumed — nothing else
            return true;
        }
    }

    // Detail overlay?
    if app.opened.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h') => app.close_detail(),
            KeyCode::Down | KeyCode::Char('j') => {
                app.detail_scroll = app.detail_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.detail_scroll = app.detail_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                app.detail_scroll = app.detail_scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                app.detail_scroll = app.detail_scroll.saturating_sub(10);
            }
            KeyCode::Char('g') => app.detail_scroll = 0,
            KeyCode::Char('d') => app.request_delete(),
            KeyCode::Char(':') => {
                app.mode = Mode::Command;
                app.command.clear();
            }
            // 1..9 — открыть ссылку с этим номером в браузере хоста.
            KeyCode::Char(c @ '1'..='9') => {
                if let Some(idx) = c.to_digit(10).map(|n| n as usize) {
                    open_overlay_link(app, idx.saturating_sub(1));
                }
            }
            _ => {}
        }
        return true;
    }

    match (app.focus, code) {
        (_, KeyCode::Char('q')) | (_, KeyCode::Esc) => return false,
        (_, KeyCode::Tab) => app.focus = app.focus.next(),
        (_, KeyCode::BackTab) => app.focus = app.focus.prev(),
        (_, KeyCode::Char(':')) => {
            app.mode = Mode::Command;
            app.command.clear();
        }
        (_, KeyCode::Char('i')) => {
            app.switch_tab(Tab::Inbox);
            app.mode = Mode::Ingest;
            app.input.clear();
        }
        (_, KeyCode::Char('/')) => {
            app.mode = Mode::Search;
            app.search.clear();
        }
        (_, KeyCode::Char('r')) => app.refresh(),
        // Quick tab switching
        (_, KeyCode::Char('1')) => app.switch_tab(Tab::Inbox),
        (_, KeyCode::Char('2')) => app.switch_tab(Tab::Notes),
        (_, KeyCode::Char('3')) => app.switch_tab(Tab::Todos),
        (_, KeyCode::Char('4')) => app.switch_tab(Tab::Reminders),
        (_, KeyCode::Char('5')) => app.switch_tab(Tab::Learning),
        (_, KeyCode::Char('6')) => app.switch_tab(Tab::Docs),
        // `]` prepares for `]t`
        (_, KeyCode::Char(']')) => {
            app.next_tab();
        }
        (_, KeyCode::Char('[')) => {
            app.prev_tab();
        }

        // ── Focus-specific ──
        (Focus::Sidebar, KeyCode::Down) | (Focus::Sidebar, KeyCode::Char('j')) => {
            app.next_tab();
        }
        (Focus::Sidebar, KeyCode::Up) | (Focus::Sidebar, KeyCode::Char('k')) => {
            app.prev_tab();
        }
        (Focus::Sidebar, KeyCode::Enter) | (Focus::Sidebar, KeyCode::Right) => {
            app.focus = Focus::List;
        }

        (Focus::List, KeyCode::Down) | (Focus::List, KeyCode::Char('j')) => app.select_next(),
        (Focus::List, KeyCode::Up) | (Focus::List, KeyCode::Char('k')) => app.select_prev(),
        (Focus::List, KeyCode::Char('g')) => app.select_first(),
        (Focus::List, KeyCode::Char('G')) => app.select_last(),
        (Focus::List, KeyCode::Enter) | (Focus::List, KeyCode::Char('o')) => app.open_selected(),
        (Focus::List, KeyCode::Char(' ')) => app.toggle_todo_done(),
        (Focus::List, KeyCode::Char('d')) | (Focus::List, KeyCode::Char('x')) => {
            app.request_delete()
        }
        (Focus::List, KeyCode::Left) | (Focus::List, KeyCode::Char('h')) => {
            app.focus = Focus::Sidebar;
        }
        (Focus::List, KeyCode::Right) | (Focus::List, KeyCode::Char('l')) => {
            app.focus = Focus::Detail;
        }

        (Focus::Detail, KeyCode::Down) | (Focus::Detail, KeyCode::Char('j')) => {
            app.detail_scroll = app.detail_scroll.saturating_add(1);
        }
        (Focus::Detail, KeyCode::Up) | (Focus::Detail, KeyCode::Char('k')) => {
            app.detail_scroll = app.detail_scroll.saturating_sub(1);
        }
        (Focus::Detail, KeyCode::PageDown) => {
            app.detail_scroll = app.detail_scroll.saturating_add(10);
        }
        (Focus::Detail, KeyCode::PageUp) => {
            app.detail_scroll = app.detail_scroll.saturating_sub(10);
        }
        (Focus::Detail, KeyCode::Left) | (Focus::Detail, KeyCode::Char('h')) => {
            app.focus = Focus::List;
        }

        _ => {}
    }
    true
}

fn handle_ingest(app: &mut App, code: KeyCode, mods: KeyModifiers) -> bool {
    let has_completions = !app.completions.is_empty();

    // Перенос строки: Ctrl+Enter / Alt+Enter. Разные терминалы по-разному
    // передают Ctrl+Enter — оба комбо покрывают ghostty/iTerm/kitty/wezterm.
    let is_newline = matches!(code, KeyCode::Enter)
        && (mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::ALT));
    if is_newline {
        app.input.push('\n');
        app.recompute_completions();
        return true;
    }

    match code {
        // Управление попапом автодополнения — активно только пока он открыт.
        KeyCode::Up if has_completions => app.completion_prev(),
        KeyCode::Down if has_completions => app.completion_next(),
        KeyCode::Tab if has_completions => {
            app.accept_completion();
        }
        // Enter при открытом попапе — принять подсказку, а не отправить.
        KeyCode::Enter if has_completions => {
            app.accept_completion();
        }
        // Esc при открытом попапе — закрыть его, ingest-режим не выходим.
        KeyCode::Esc if has_completions => {
            app.completions.clear();
            app.completion_index = 0;
        }

        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input.clear();
            app.completions.clear();
        }
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input).trim().to_string();
            app.mode = Mode::Normal;
            app.completions.clear();
            if !text.is_empty() {
                app.loading = true;
                app.status = "LLM обрабатывает…".into();
                let _ = app.cmd_tx.send(AppCmd::Ingest(text));
            }
        }
        KeyCode::Tab => {
            app.input.push('\t');
            app.recompute_completions();
        }
        KeyCode::Char(c) => {
            app.input.push(c);
            app.recompute_completions();
        }
        KeyCode::Backspace => {
            app.input.pop();
            app.recompute_completions();
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

/// Открывает ссылку с индексом `idx` (0-based) из текущего opened-объекта,
/// если это note/doc-страница с markdown-content и в нём есть линки.
fn open_overlay_link(app: &mut App, idx: usize) {
    let Some(v) = app.opened.as_ref() else { return };
    let is_md = matches!(app.tab, Tab::Notes | Tab::Docs);
    if !is_md {
        return;
    }
    let md = v["content"].as_str().unwrap_or("");
    let r = markdown::render_with_links(md);
    let Some(url) = r.links.get(idx) else { return };
    match markdown::open_url(url) {
        Ok(()) => app.status = format!("→ открыто [{}] {url}", idx + 1),
        Err(e) => app.status = format!("не открыть [{}]: {e}", idx + 1),
    }
}

fn handle_command(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.command.clear();
        }
        KeyCode::Enter => {
            let cmd = std::mem::take(&mut app.command);
            let is_quit = matches!(cmd.as_str(), "q" | "quit" | "exit");
            if !is_quit {
                app.run_command(&cmd);
                app.mode = Mode::Normal;
            }
            if is_quit {
                return false;
            }
        }
        KeyCode::Char(c) => app.command.push(c),
        KeyCode::Backspace => {
            app.command.pop();
        }
        _ => {}
    }
    true
}

fn handle_confirm(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            app.confirm_yes();
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.confirm_no();
        }
        _ => {}
    }
    true
}
