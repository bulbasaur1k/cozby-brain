//! Rendering.
//!
//! Раскладка:
//! ┌──────────────────────────────────────────────────────────────┐
//! │ header: cozby · http://… · connected                         │
//! ├─────────┬──────────────────────┬─────────────────────────────┤
//! │ sidebar │   list                │  detail / inbox preview    │
//! │ (tabs)  │                       │                             │
//! │         │                       │                             │
//! ├─────────┴──────────────────────┴─────────────────────────────┤
//! │ status: mode · spinner · message · keybinding hints          │
//! └──────────────────────────────────────────────────────────────┘

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

use crate::app::{App, Mode, Tab};
use crate::indicators;
use crate::theme;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    // header / body / status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(f, app, chunks[0]);
    render_body(f, app, chunks[1]);
    render_statusbar(f, app, chunks[2]);
}

// ─── header ──────────────────────────────────────────────────────────

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let (dot, dot_style) = if app.connected {
        ("●", theme::ok())
    } else {
        ("●", theme::error())
    };
    let line = Line::from(vec![
        Span::styled("  cozby", theme::accent()),
        Span::styled(" · ", theme::overlay()),
        Span::styled(app.api.clone(), theme::subtext()),
        Span::styled("  ", theme::overlay()),
        Span::styled(dot, dot_style),
        Span::styled(
            if app.connected { " connected" } else { " offline" },
            theme::subtext(),
        ),
    ]);
    let p = Paragraph::new(line).style(Style::default().bg(theme::MANTLE));
    f.render_widget(p, area);
}

// ─── body = sidebar + list + detail ─────────────────────────────────

fn render_body(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18),
            Constraint::Percentage(50),
            Constraint::Min(40),
        ])
        .split(area);

    render_sidebar(f, app, chunks[0]);
    render_main(f, app, chunks[1]);
    render_detail(f, app, chunks[2]);
}

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let active = *t == app.tab;
            let icon = if active { "▎" } else { " " };
            let icon_style = if active { theme::accent() } else { theme::overlay() };
            let label_style = if active {
                theme::active_tab()
            } else {
                theme::inactive_tab()
            };
            ListItem::new(Line::from(vec![
                Span::styled(icon, icon_style),
                Span::raw(" "),
                Span::styled(format!("{} ", i + 1), theme::overlay()),
                Span::styled(t.label(), label_style),
            ]))
        })
        .collect();

    let list = List::new(items).block(theme::block("cozby"));
    f.render_widget(list, area);
}

fn render_main(f: &mut Frame, app: &App, area: Rect) {
    if app.tab == Tab::Inbox {
        render_inbox(f, app, area);
        return;
    }

    // Список текущей вкладки с фильтром
    let items = app.filtered_items();
    let rendered: Vec<ListItem> = items
        .iter()
        .map(|v| ListItem::new(row_for_tab(app.tab, v)))
        .collect();

    let title = if !app.search.is_empty() {
        format!(
            "{}  (/ {}  · {} of {})",
            app.tab.label(),
            app.search,
            items.len(),
            app.items.len()
        )
    } else {
        format!("{}  ({})", app.tab.label(), app.items.len())
    };

    let mut list = List::new(rendered)
        .block(theme::block(&title))
        .highlight_style(theme::selected_row())
        .highlight_symbol("▶ ");

    if app.loading {
        list = list.block(theme::block(&format!(
            "{}  {}",
            title,
            app.spinner_frame()
        )));
    }

    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.selected.min(items.len() - 1)));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_inbox(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // input box
            Constraint::Min(1),    // preview of last ingest
        ])
        .split(area);

    // input
    let label = if app.mode == Mode::Ingest {
        "ingest (Enter → send · Esc → cancel)"
    } else {
        "ingest (i → edit · / → search last · r → refresh)"
    };
    let input = Paragraph::new(app.input.as_str()).block(if app.mode == Mode::Ingest {
        theme::block_focused(label)
    } else {
        theme::block(label)
    });
    f.render_widget(input, rows[0]);

    // preview
    let preview_text = if let Some(v) = &app.last_ingest {
        format_ingest_result(v)
    } else {
        welcome_message()
    };
    let preview = Paragraph::new(preview_text)
        .wrap(Wrap { trim: false })
        .block(theme::block("preview"))
        .style(theme::text());
    f.render_widget(preview, rows[1]);
}

fn welcome_message() -> String {
    "Пиши что угодно — LLM классифицирует автоматически:\n\
     \n\
     note     — факт/мысль/документация\n\
     doc      — страница в проекте: \"в проекте X на страницу Y…\"\n\
     todo     — действие: \"надо сделать…\"\n\
     reminder — с временем: \"через 30 минут…\"\n\
     question — поиск: \"что я писал про…\"\n\
     \n\
     В одном сообщении можно смешивать разные типы и проекты."
        .into()
}

// ─── detail panel ────────────────────────────────────────────────────

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    if app.tab == Tab::Inbox {
        render_help(f, area);
        return;
    }

    let items = app.filtered_items();
    let selected = items.get(app.selected);

    let title = "details";
    let text = match selected {
        Some(v) => format_detail(app.tab, v),
        None => "(нет выбранного элемента)".into(),
    };

    let p = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .block(theme::block(title))
        .style(theme::text());
    f.render_widget(p, area);
}

fn render_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled("Клавиши", theme::accent())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  h/l Tab ← → ", theme::info()),
            Span::styled("переключить вкладку", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  j/k ↓ ↑      ", theme::info()),
            Span::styled("навигация", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  g / G        ", theme::info()),
            Span::styled("к началу / к концу", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  1-6          ", theme::info()),
            Span::styled("прямой переход на вкладку", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  i            ", theme::info()),
            Span::styled("ingest (писать в LLM)", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  /            ", theme::info()),
            Span::styled("фильтр по списку", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  r            ", theme::info()),
            Span::styled("обновить", theme::subtext()),
        ]),
        Line::from(vec![
            Span::styled("  q / Esc      ", theme::info()),
            Span::styled("выход", theme::subtext()),
        ]),
        Line::from(""),
        Line::from(Span::styled("Советы", theme::accent())),
        Line::from(""),
        Line::from(Span::styled(
            "  Напоминания срабатывают автоматически",
            theme::subtext(),
        )),
        Line::from(Span::styled(
            "  при наступлении времени → popup + звук.",
            theme::subtext(),
        )),
        Line::from(Span::styled(
            "  Бэкенд должен быть запущен (./run.sh).",
            theme::subtext(),
        )),
    ];
    let p = Paragraph::new(lines).block(theme::block("cheatsheet"));
    f.render_widget(p, area);
}

// ─── status bar ──────────────────────────────────────────────────────

fn render_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let mode = match app.mode {
        Mode::Normal => ("NORMAL", theme::info()),
        Mode::Ingest => ("INGEST", theme::warn()),
        Mode::Search => ("SEARCH", theme::link()),
    };

    let hint = match app.mode {
        Mode::Normal => "h/l tabs · j/k nav · i write · / search · r refresh · q quit",
        Mode::Ingest => "Enter → send   Esc → cancel",
        Mode::Search => "type to filter · Esc cancel · Enter apply",
    };

    let mut spans = vec![
        Span::styled(format!(" {} ", mode.0), mode.1.add_modifier(Modifier::BOLD)),
        Span::styled(" ", theme::text()),
    ];
    if app.loading {
        spans.push(Span::styled(
            format!("{} ", app.spinner_frame()),
            theme::warn(),
        ));
    }
    spans.push(Span::styled(app.status.clone(), theme::subtext()));
    spans.push(Span::styled(" · ", theme::overlay()));
    spans.push(Span::styled(hint, theme::overlay()));

    let p = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::MANTLE));
    f.render_widget(p, area);
}

// ─── rows for each tab ──────────────────────────────────────────────

fn row_for_tab(tab: Tab, v: &Value) -> Line<'static> {
    match tab {
        Tab::Notes => note_row(v),
        Tab::Todos => todo_row(v),
        Tab::Reminders => reminder_row(v),
        Tab::Learning => track_row(v),
        Tab::Docs => project_row(v),
        Tab::Inbox => Line::from(""),
    }
}

fn note_row(v: &Value) -> Line<'static> {
    let title = truncate(v["title"].as_str().unwrap_or(""), 36);
    let tags = v["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    Line::from(vec![
        Span::styled(indicators::DOT_FILLED, theme::info()),
        Span::raw(" "),
        Span::styled(format!("{title:<36}"), theme::text()),
        Span::styled(format!(" {tags}"), theme::link()),
    ])
}

fn todo_row(v: &Value) -> Line<'static> {
    let done = v["done"].as_bool().unwrap_or(false);
    let (ind, ind_style) = if done {
        (indicators::CHECK, theme::ok())
    } else {
        (indicators::SQ_EMPTY, theme::warn())
    };
    let title_style = if done { theme::overlay() } else { theme::text() };
    let title = truncate(v["title"].as_str().unwrap_or(""), 40);
    let due = v["due_at"].as_str().unwrap_or("").to_string();
    Line::from(vec![
        Span::styled(ind, ind_style),
        Span::raw(" "),
        Span::styled(format!("{title:<40}"), title_style),
        Span::styled(due, theme::overlay()),
    ])
}

fn reminder_row(v: &Value) -> Line<'static> {
    let fired = v["fired"].as_bool().unwrap_or(false);
    let (ind, ind_style) = if fired {
        (indicators::DOT_FILLED, theme::ok())
    } else {
        (indicators::DOT_EMPTY, theme::warn())
    };
    let text = truncate(v["text"].as_str().unwrap_or(""), 40);
    let at = v["remind_at"].as_str().unwrap_or("").to_string();
    Line::from(vec![
        Span::styled(ind, ind_style),
        Span::raw(" "),
        Span::styled(format!("{text:<40}"), theme::text()),
        Span::styled(at, theme::overlay()),
    ])
}

fn track_row(v: &Value) -> Line<'static> {
    let title = truncate(v["title"].as_str().unwrap_or(""), 30);
    let cur = v["current_lesson"].as_i64().unwrap_or(0);
    let total = v["total_lessons"].as_i64().unwrap_or(0);
    let pace = v["pace_hours"].as_i64().unwrap_or(0);
    Line::from(vec![
        Span::styled(indicators::SQ_FILLED, theme::info()),
        Span::raw(" "),
        Span::styled(format!("{title:<30}"), theme::text()),
        Span::styled(format!(" {cur}/{total}"), theme::warn()),
        Span::styled(format!("  каждые {pace}ч"), theme::overlay()),
    ])
}

fn project_row(v: &Value) -> Line<'static> {
    let slug = v["slug"].as_str().unwrap_or("");
    let title = truncate(v["title"].as_str().unwrap_or(""), 36);
    Line::from(vec![
        Span::styled(indicators::SQ_FILLED, theme::link()),
        Span::raw(" "),
        Span::styled(format!("{slug:<18}"), theme::accent()),
        Span::raw(" "),
        Span::styled(title, theme::text()),
    ])
}

// ─── detail formatters ───────────────────────────────────────────────

fn format_detail(tab: Tab, v: &Value) -> String {
    match tab {
        Tab::Notes => {
            format!(
                "# {}\n\ntags: {}\nid: {}\n\n{}",
                v["title"].as_str().unwrap_or(""),
                v["tags"]
                    .as_array()
                    .map(|a| a
                        .iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default(),
                v["id"].as_str().unwrap_or(""),
                v["content"].as_str().unwrap_or("")
            )
        }
        Tab::Todos => format!(
            "{}\n\ndone: {}\ndue: {}\nid: {}",
            v["title"].as_str().unwrap_or(""),
            v["done"].as_bool().unwrap_or(false),
            v["due_at"].as_str().unwrap_or("—"),
            v["id"].as_str().unwrap_or("")
        ),
        Tab::Reminders => format!(
            "{}\n\nremind_at: {}\nfired: {}\nid: {}",
            v["text"].as_str().unwrap_or(""),
            v["remind_at"].as_str().unwrap_or(""),
            v["fired"].as_bool().unwrap_or(false),
            v["id"].as_str().unwrap_or("")
        ),
        Tab::Learning => format!(
            "# {}\n\nlessons: {}/{}\npace: каждые {} ч\nsource: {}\nid: {}",
            v["title"].as_str().unwrap_or(""),
            v["current_lesson"].as_i64().unwrap_or(0),
            v["total_lessons"].as_i64().unwrap_or(0),
            v["pace_hours"].as_i64().unwrap_or(0),
            v["source_ref"].as_str().unwrap_or(""),
            v["id"].as_str().unwrap_or("")
        ),
        Tab::Docs => format!(
            "# {}  ({})\n\n{}\n\nid: {}",
            v["title"].as_str().unwrap_or(""),
            v["slug"].as_str().unwrap_or(""),
            v["description"].as_str().unwrap_or(""),
            v["id"].as_str().unwrap_or("")
        ),
        Tab::Inbox => String::new(),
    }
}

fn format_ingest_result(v: &Value) -> String {
    // Multi-item?
    if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
        let mut out = format!("[+] classified {} items:\n\n", items.len());
        for (i, it) in items.iter().enumerate() {
            let kind = it["type"].as_str().unwrap_or("?");
            let title = it["data"]["title"]
                .as_str()
                .or_else(|| it["data"]["text"].as_str())
                .or_else(|| it["structured"]["title"].as_str())
                .unwrap_or("?");
            out.push_str(&format!("  {}. [{}] {}\n", i + 1, kind, title));
        }
        return out;
    }
    let kind = v["type"].as_str().unwrap_or("?");
    let title = v["data"]["title"]
        .as_str()
        .or_else(|| v["data"]["text"].as_str())
        .or_else(|| v["structured"]["title"].as_str())
        .unwrap_or("?");
    format!("[+] {kind}: {title}")
}

// ─── helpers ─────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars.into_iter().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
