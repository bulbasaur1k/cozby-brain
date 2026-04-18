//! Rendering entry point.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;
use serde_json::Value;

use crate::app::{App, Tab};
use crate::indicators;

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tabs
            Constraint::Min(1),     // body
            Constraint::Length(3),  // status + hints
        ])
        .split(f.area());

    render_tabs(f, app, chunks[0]);
    render_body(f, app, chunks[1]);
    render_status(f, app, chunks[2]);
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| Line::from(Span::raw(t.label())))
        .collect();
    let idx = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" cozby-brain "))
        .select(idx)
        .style(indicators::dim())
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(ratatui::style::Color::White),
        );
    f.render_widget(tabs, area);
}

fn render_body(f: &mut Frame, app: &App, area: Rect) {
    match app.tab {
        Tab::Inbox => render_inbox(f, app, area),
        Tab::Notes => render_list(f, app, area, note_line),
        Tab::Todos => render_list(f, app, area, todo_line),
        Tab::Reminders => render_list(f, app, area, reminder_line),
        Tab::Learning => render_list(f, app, area, track_line),
    }
}

fn render_inbox(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Input box
    let input_label = if app.input_mode {
        " ввод (Enter — отправить, Esc — отмена) "
    } else {
        " (нажми 'i' чтобы писать) "
    };
    let input = Paragraph::new(app.input.as_str())
        .style(if app.input_mode {
            Style::default().fg(ratatui::style::Color::White)
        } else {
            indicators::dim()
        })
        .block(Block::default().borders(Borders::ALL).title(input_label));
    f.render_widget(input, chunks[0]);

    // Last ingest preview
    let preview = if let Some(v) = &app.last_ingest {
        format_ingest_result(v)
    } else {
        "Пиши что угодно — LLM сама решит: note / todo / reminder / question.\n\n\
         Примеры:\n\
         - 'разбирался с ractor 0.15, убрали async_trait' → note\n\
         - 'надо купить молоко завтра в 10' → todo\n\
         - 'через 30 минут позвонить маме' → reminder\n\
         - 'что я писал про rust' → question (поиск)"
            .to_string()
    };
    let body = Paragraph::new(preview)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" последний результат "));
    f.render_widget(body, chunks[1]);
}

fn format_ingest_result(v: &Value) -> String {
    let kind = v["type"].as_str().unwrap_or("?");
    match kind {
        "note" => {
            let s = &v["structured"];
            let title = s["title"].as_str().unwrap_or("");
            let content = s["content"].as_str().unwrap_or("");
            let tags = s["tags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let mut out = format!("[NOTE]\n\n{title}\n");
            if !tags.is_empty() {
                out.push_str(&format!("теги: {tags}\n"));
            }
            out.push('\n');
            out.push_str(content);
            if !v["suggestion"].is_null() {
                let s_title = v["suggestion"]["target_title"].as_str().unwrap_or("?");
                let s_score = v["suggestion"]["score"].as_f64().unwrap_or(0.0);
                out.push_str(&format!(
                    "\n\n(найдена похожая: \"{s_title}\", score {s_score:.2})"
                ));
            }
            out
        }
        "todo" => {
            let t = &v["data"];
            format!(
                "[TODO]\n\n{}\ndue: {}",
                t["title"].as_str().unwrap_or(""),
                t["due_at"].as_str().unwrap_or("-")
            )
        }
        "reminder" => {
            let r = &v["data"];
            format!(
                "[REMINDER]\n\n{}\nat: {}",
                r["text"].as_str().unwrap_or(""),
                r["remind_at"].as_str().unwrap_or("")
            )
        }
        "question" => {
            let kw = v["keywords"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let d = &v["data"];
            let nn = d["notes"].as_array().map(|a| a.len()).unwrap_or(0);
            let nt = d["todos"].as_array().map(|a| a.len()).unwrap_or(0);
            let nr = d["reminders"].as_array().map(|a| a.len()).unwrap_or(0);
            format!(
                "[QUESTION]\n\nключевые: {kw}\n\nnotes: {nn}  todos: {nt}  reminders: {nr}"
            )
        }
        _ => format!("{v:#}"),
    }
}

fn render_list<F>(f: &mut Frame, app: &App, area: Rect, line_fn: F)
where
    F: Fn(&Value) -> Line<'static>,
{
    let items: Vec<ListItem> = app.items.iter().map(|v| ListItem::new(line_fn(v))).collect();
    let title = format!(" {} ({}) ", app.tab.label(), app.items.len());
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    state.select(Some(app.selected.min(app.items.len().saturating_sub(1))));
    f.render_stateful_widget(list, area, &mut state);
}

fn note_line(v: &Value) -> Line<'static> {
    let id = v["id"].as_str().unwrap_or("").to_string();
    let title = v["title"].as_str().unwrap_or("").to_string();
    let tags = v["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Line::from(vec![
        Span::styled(indicators::DOT_FILLED, indicators::info()),
        Span::raw("  "),
        Span::styled(format!("{:<40}  ", truncate(&title, 40)), Style::default()),
        Span::styled(format!("[{}]", short_id(&id)), indicators::dim()),
        Span::raw("  "),
        Span::styled(tags, indicators::link()),
    ])
}

fn todo_line(v: &Value) -> Line<'static> {
    let done = v["done"].as_bool().unwrap_or(false);
    let (ind, style) = indicators::todo_status(done);
    let id = v["id"].as_str().unwrap_or("").to_string();
    let title = v["title"].as_str().unwrap_or("").to_string();
    let due = v["due_at"].as_str().unwrap_or("").to_string();
    Line::from(vec![
        Span::styled(ind, style),
        Span::raw("  "),
        Span::raw(format!("{:<40}  ", truncate(&title, 40))),
        Span::styled(format!("[{}]", short_id(&id)), indicators::dim()),
        Span::raw("  "),
        Span::styled(due, indicators::dim()),
    ])
}

fn reminder_line(v: &Value) -> Line<'static> {
    let fired = v["fired"].as_bool().unwrap_or(false);
    let (ind, style) = indicators::reminder_status(fired);
    let id = v["id"].as_str().unwrap_or("").to_string();
    let text = v["text"].as_str().unwrap_or("").to_string();
    let at = v["remind_at"].as_str().unwrap_or("").to_string();
    Line::from(vec![
        Span::styled(ind, style),
        Span::raw("  "),
        Span::raw(format!("{:<40}  ", truncate(&text, 40))),
        Span::styled(format!("[{}]", short_id(&id)), indicators::dim()),
        Span::raw("  "),
        Span::styled(at, indicators::dim()),
    ])
}

fn track_line(v: &Value) -> Line<'static> {
    let id = v["id"].as_str().unwrap_or("").to_string();
    let title = v["title"].as_str().unwrap_or("").to_string();
    let cur = v["current_lesson"].as_i64().unwrap_or(0);
    let total = v["total_lessons"].as_i64().unwrap_or(0);
    let pace = v["pace_hours"].as_i64().unwrap_or(0);
    Line::from(vec![
        Span::styled(indicators::SQ_FILLED, indicators::info()),
        Span::raw("  "),
        Span::raw(format!("{:<30}  ", truncate(&title, 30))),
        Span::styled(format!("{cur:>3}/{total:<3}"), indicators::medium()),
        Span::raw("  "),
        Span::styled(format!("каждые {pace} ч"), indicators::dim()),
        Span::raw("  "),
        Span::styled(format!("[{}]", short_id(&id)), indicators::dim()),
    ])
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let hint = if app.input_mode {
        "Enter — отправить  Esc — отмена"
    } else {
        "Tab — след. вкладка  Shift+Tab — пред.  i — ввод  r — обновить  ↑↓ — навигация  q — выход"
    };
    let content = if app.status.is_empty() {
        hint.to_string()
    } else {
        format!("{}  |  {}", hint, app.status)
    };
    let p = Paragraph::new(content)
        .style(indicators::dim())
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let truncated: String = chars.into_iter().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

fn short_id(s: &str) -> String {
    s.chars().take(8).collect()
}
