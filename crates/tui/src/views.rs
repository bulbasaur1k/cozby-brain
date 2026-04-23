//! Rendering.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

use crate::app::{App, DocRow, Focus, Mode, PendingConfirm, Tab, TodoFilter};
use crate::indicators;
use crate::markdown;
use crate::theme;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

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

    // Overlays
    if app.opened.is_some() {
        render_detail_overlay(f, app, area);
    }
    if app.mode == Mode::Confirm {
        render_confirm(f, app, area);
    }
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
    // В ingest-режиме на Inbox cheatsheet справа не нужен — даём всё место
    // под ввод. В любом другом случае сохраняем 3-колоночную раскладку.
    let full_width_ingest = app.tab == Tab::Inbox && app.mode == Mode::Ingest;

    if full_width_ingest {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(18), Constraint::Min(40)])
            .split(area);
        render_sidebar(f, app, chunks[0]);
        render_main(f, app, chunks[1]);
    } else {
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

    let block = if app.focus == Focus::Sidebar {
        theme::block_focused("cozby")
    } else {
        theme::block("cozby")
    };
    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_main(f: &mut Frame, app: &App, area: Rect) {
    if app.tab == Tab::Inbox {
        render_inbox(f, app, area);
        return;
    }

    if app.tab == Tab::Docs {
        render_docs_tree(f, app, area);
        return;
    }

    let items = app.filtered_items();
    let rendered: Vec<ListItem> = items
        .iter()
        .map(|v| ListItem::new(row_for_tab(app.tab, v)))
        .collect();

    let mut title = format!("{}  ({})", app.tab.label(), app.items.len());
    if !app.search.is_empty() {
        title = format!(
            "{}  / {}  ({}/{})",
            app.tab.label(),
            app.search,
            items.len(),
            app.items.len()
        );
    }
    if app.tab == Tab::Todos {
        if let TodoFilter::LastDays(n) = app.todo_filter {
            title.push_str(&format!("  · last {n}d"));
        } else {
            title.push_str("  · all");
        }
    }
    if app.loading {
        title.push_str(&format!("  {}", app.spinner_frame()));
    }

    let block = if app.focus == Focus::List {
        theme::block_focused(&title)
    } else {
        theme::block(&title)
    };

    let list = List::new(rendered)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.selected.min(items.len() - 1)));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_docs_tree(f: &mut Frame, app: &App, area: Rect) {
    let rows = app.docs_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            DocRow::Project {
                value,
                expanded,
                page_count,
            } => {
                let chevron = if *expanded { "▼" } else { "▶" };
                let slug = value["slug"].as_str().unwrap_or("");
                let title = value["title"].as_str().unwrap_or("");
                ListItem::new(Line::from(vec![
                    Span::styled(chevron, theme::accent()),
                    Span::raw(" "),
                    Span::styled(indicators::SQ_FILLED, theme::link()),
                    Span::raw(" "),
                    Span::styled(slug.to_string(), theme::accent()),
                    Span::styled("  ", theme::overlay()),
                    Span::styled(title.to_string(), theme::text()),
                    Span::styled(format!("  ({page_count})"), theme::overlay()),
                ]))
            }
            DocRow::Page { value, .. } => {
                let title = value["title"].as_str().unwrap_or("");
                let v = value["version"].as_i64().unwrap_or(1);
                ListItem::new(Line::from(vec![
                    Span::styled("    ", theme::overlay()),
                    Span::styled(indicators::DOT_EMPTY, theme::subtext()),
                    Span::raw(" "),
                    Span::styled(title.to_string(), theme::text()),
                    Span::styled(format!("  v{v}"), theme::overlay()),
                ]))
            }
        })
        .collect();

    let title = format!("Docs  ({} projects)", app.items.len());
    let block = if app.focus == Focus::List {
        theme::block_focused(&title)
    } else {
        theme::block(&title)
    };
    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol("▶ ");
    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(app.selected.min(rows.len() - 1)));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_inbox(f: &mut Frame, app: &App, area: Rect) {
    let is_ingesting = app.mode == Mode::Ingest;

    // В режиме ввода отдаём input почти весь экран — длинные тексты должны
    // помещаться без того чтобы каретка уезжала за правый край. В obычном
    // режиме оставляем старые 3 строки + preview.
    let rows = if is_ingesting {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area)
    };

    render_ingest_input(f, app, rows[0], is_ingesting);

    let preview_text = if is_ingesting {
        // Маленькая подсказка под полем ввода — как отправить/отменить.
        "Ctrl+Enter / Alt+Enter / Ctrl+D — отправить   ·   Esc — отменить\n\
         Enter вставляет перенос строки · @/path/to/file — прочитать файл"
            .to_string()
    } else if let Some(v) = &app.last_ingest {
        format_ingest_result(v)
    } else {
        welcome_message()
    };
    let preview = Paragraph::new(preview_text)
        .wrap(Wrap { trim: false })
        .block(theme::block(if is_ingesting { "hint" } else { "preview" }))
        .style(theme::text());
    f.render_widget(preview, rows[1]);
}

/// Поле ингеста: многострочный ввод с визуальным переносом, вертикальным
/// скроллом (чтобы каретка всегда была видна) и миганием курсора в терминале.
fn render_ingest_input(f: &mut Frame, app: &App, area: Rect, is_ingesting: bool) {
    let label = if is_ingesting {
        "ingest (Ctrl+Enter → send · Esc → cancel · @/path для файла)"
    } else {
        "ingest (i → edit · @/path для файла)"
    };
    let block = if is_ingesting {
        theme::block_focused(label)
    } else {
        theme::block(label)
    };

    // Inner rect (внутри рамки) нужен чтобы вычислить ширину переноса и
    // позицию курсора в координатах терминала.
    let inner = block.inner(area);
    let inner_w = inner.width as usize;
    let inner_h = inner.height.max(1) as usize;

    let (total_rows, cur_row, cur_col) = wrap_cursor_pos(&app.input, inner_w);
    // Скролл — минимальный такой, чтобы курсор (или последняя строка) были
    // в поле зрения.
    let scroll = cur_row
        .saturating_sub(inner_h.saturating_sub(1))
        .max(total_rows.saturating_sub(inner_h));

    let para = Paragraph::new(app.input.as_str())
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0))
        .block(block);
    f.render_widget(para, area);

    if is_ingesting && inner_w > 0 && inner.height > 0 {
        let visible_row = cur_row.saturating_sub(scroll);
        let cx = inner.x + (cur_col.min(inner_w.saturating_sub(1))) as u16;
        let cy = inner.y + (visible_row.min(inner_h.saturating_sub(1))) as u16;
        f.set_cursor_position((cx, cy));
    }
}

/// Простая симуляция переноса для фиксированной ширины: возвращает
/// `(всего_строк, строка_курсора, колонка_курсора)` для каретки в конце
/// текста. Считает `char` за 1 колонку — для кириллицы/латиницы верно,
/// для широких символов (CJK/emoji) — приближение.
fn wrap_cursor_pos(text: &str, width: usize) -> (usize, usize, usize) {
    if width == 0 {
        return (1, 0, 0);
    }
    let mut row = 0usize;
    let mut col = 0usize;
    for c in text.chars() {
        if c == '\n' {
            row += 1;
            col = 0;
            continue;
        }
        if col >= width {
            row += 1;
            col = 0;
        }
        col += 1;
    }
    (row + 1, row, col)
}

fn welcome_message() -> String {
    "Пиши что угодно — LLM классифицирует:\n\
     \n\
     note     — мысль / факт\n\
     doc      — страница в проекте (\"в проекте X…\")\n\
     todo     — действие (\"надо…\")\n\
     reminder — с временем (\"через 30 мин…\")\n\
     question — поиск\n\
     \n\
     В одном сообщении можно смешивать разные типы.\n\
     \n\
     Файл: начни с @/путь/к/файлу (или @~/file.md) — TUI прочитает\n\
     его и отправит содержимое. Можно дописать контекст после пути."
        .into()
}

// ─── detail panel (right side, always visible) ──────────────────────

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    if app.tab == Tab::Inbox {
        render_help(f, app, area);
        return;
    }

    let title = "preview";
    let block = if app.focus == Focus::Detail {
        theme::block_focused(title)
    } else {
        theme::block(title)
    };

    let (content_for_md, fallback_text): (Option<String>, String) = match app.tab {
        Tab::Notes => {
            let v = app.filtered_items();
            match v.get(app.selected) {
                Some(n) => {
                    let md = n["content"].as_str().unwrap_or("").to_string();
                    (Some(md), format_detail_meta_note(n))
                }
                None => (None, "(пусто)".into()),
            }
        }
        Tab::Docs => {
            let rows = app.docs_rows();
            match rows.get(app.selected) {
                Some(DocRow::Project { value, .. }) => (
                    None,
                    format!(
                        "# {}\n\n{}\n\nid: {}\nslug: {}",
                        value["title"].as_str().unwrap_or(""),
                        value["description"].as_str().unwrap_or(""),
                        value["id"].as_str().unwrap_or(""),
                        value["slug"].as_str().unwrap_or("")
                    ),
                ),
                Some(DocRow::Page { value, .. }) => {
                    let md = value["content"].as_str().unwrap_or("").to_string();
                    (Some(md), format_detail_meta_page(value))
                }
                None => (None, "(пусто)".into()),
            }
        }
        _ => {
            let items = app.filtered_items();
            match items.get(app.selected) {
                Some(v) => (None, format_detail(app.tab, v)),
                None => (None, "(пусто)".into()),
            }
        }
    };

    match content_for_md {
        Some(md) => {
            let text = markdown::render(&md);
            let p = Paragraph::new(text).wrap(Wrap { trim: false }).block(block);
            f.render_widget(p, area);
            let _ = fallback_text;
        }
        None => {
            let p = Paragraph::new(fallback_text)
                .wrap(Wrap { trim: false })
                .block(block)
                .style(theme::text());
            f.render_widget(p, area);
        }
    }
}

fn render_help(f: &mut Frame, app: &App, area: Rect) {
    let _ = app;
    let lines = vec![
        Line::from(Span::styled("Клавиши", theme::accent())),
        Line::from(""),
        key_line("Tab / Shift+Tab", "цикл фокуса"),
        key_line("1 - 6  ·  [t / ]t", "переключение вкладок"),
        key_line("j / k  ·  ↓ ↑", "навигация"),
        key_line("g / G", "к началу / к концу"),
        key_line("Enter / o", "открыть запись (раскрыть проект)"),
        key_line("Space", "toggle done (todo)"),
        key_line("d / x", "удалить (y/n подтверждение)"),
        key_line("i", "ingest — писать в LLM"),
        key_line("/", "фильтр списка"),
        key_line(":", "командный режим"),
        key_line("r", "обновить"),
        key_line("Esc  ·  q", "закрыть detail / выход"),
        Line::from(""),
        Line::from(Span::styled("Команды (:)", theme::accent())),
        Line::from(""),
        key_line(":notes :docs :todos …", "сменить вкладку"),
        key_line(":open :delete :close", "действия с записью"),
        key_line(":all :recent", "фильтр todo (все / 5 дней)"),
        key_line(":q", "выход"),
        Line::from(""),
        Line::from(Span::styled("Советы", theme::accent())),
        Line::from(""),
        Line::from(Span::styled(
            "• Напоминания → popup + звук при сроке",
            theme::subtext(),
        )),
        Line::from(Span::styled(
            "• Todo показывает последние 5 дней",
            theme::subtext(),
        )),
        Line::from(Span::styled(
            "• Docs: проекты раскрываются как папки",
            theme::subtext(),
        )),
    ];
    let p = Paragraph::new(lines).block(theme::block("cheatsheet"));
    f.render_widget(p, area);
}

fn key_line(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<22}"), theme::info()),
        Span::styled(desc.to_string(), theme::subtext()),
    ])
}

// ─── overlays ────────────────────────────────────────────────────────

fn render_detail_overlay(f: &mut Frame, app: &App, area: Rect) {
    let Some(v) = app.opened.as_ref() else { return };
    let popup = centered_rect(90, 85, area);
    f.render_widget(Clear, popup);

    // Content: markdown if note/page, else structured text
    let is_md = app.tab == Tab::Notes || app.tab == Tab::Docs;
    let body = if is_md {
        let md = v["content"].as_str().unwrap_or("");
        markdown::render(md)
    } else {
        Text::from(format_detail(app.tab, v))
    };

    let title = v["title"]
        .as_str()
        .or_else(|| v["text"].as_str())
        .unwrap_or("detail");
    let title_full = format!("{title}  (Esc close · d delete · j/k scroll)");

    let p = Paragraph::new(body)
        .wrap(Wrap { trim: false })
        .block(theme::block_focused(&title_full))
        .scroll((app.detail_scroll, 0));
    f.render_widget(p, popup);
}

fn render_confirm(f: &mut Frame, app: &App, area: Rect) {
    let label = match &app.confirm {
        Some(PendingConfirm::Delete(_, _, l)) => format!("Удалить {l}?"),
        None => return,
    };
    let popup = centered_rect(50, 20, area);
    f.render_widget(Clear, popup);
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(label, theme::warn())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y] ", theme::ok()),
            Span::styled("да    ", theme::text()),
            Span::styled("[n] ", theme::error()),
            Span::styled("отмена", theme::text()),
        ]),
    ];
    let p = Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(theme::block_focused("confirm"));
    f.render_widget(p, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

// ─── status bar ──────────────────────────────────────────────────────

fn render_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let (mode_str, mode_style) = match app.mode {
        Mode::Normal => ("NORMAL", theme::info()),
        Mode::Ingest => ("INGEST", theme::warn()),
        Mode::Search => ("SEARCH", theme::link()),
        Mode::Command => ("CMD", theme::accent()),
        Mode::Confirm => ("CONFIRM", theme::error()),
    };

    let mut spans = vec![Span::styled(
        format!(" {mode_str} "),
        mode_style.add_modifier(Modifier::BOLD),
    )];

    // Command input appears in the status bar
    if app.mode == Mode::Command {
        spans.push(Span::styled(" :", theme::accent()));
        spans.push(Span::styled(app.command.clone(), theme::text()));
        spans.push(Span::styled("_", theme::accent()));
    } else {
        spans.push(Span::raw(" "));
        if app.loading {
            spans.push(Span::styled(
                format!("{} ", app.spinner_frame()),
                theme::warn(),
            ));
        }
        spans.push(Span::styled(app.status.clone(), theme::subtext()));
        spans.push(Span::styled(" · ", theme::overlay()));
        spans.push(Span::styled(hint_for_mode(app.mode), theme::overlay()));
    }

    let p = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::MANTLE));
    f.render_widget(p, area);
}

fn hint_for_mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Normal => "Tab focus · 1-6 tabs · j/k nav · Enter open · Space done · d delete · : cmd · / search · i write · q quit",
        Mode::Ingest => "Enter → send · Esc → cancel",
        Mode::Search => "type to filter · Esc cancel · Enter apply",
        Mode::Command => "Enter → run · Esc → cancel",
        Mode::Confirm => "y → yes · n / Esc → cancel",
    }
}

// ─── rows per tab ────────────────────────────────────────────────────

fn row_for_tab(tab: Tab, v: &Value) -> Line<'static> {
    match tab {
        Tab::Notes => note_row(v),
        Tab::Todos => todo_row(v),
        Tab::Reminders => reminder_row(v),
        Tab::Learning => track_row(v),
        Tab::Docs | Tab::Inbox => Line::from(""),
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
    let title_style = if done {
        theme::overlay().add_modifier(Modifier::CROSSED_OUT)
    } else {
        theme::text()
    };
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

// ─── detail formatters (non-markdown) ───────────────────────────────

fn format_detail(tab: Tab, v: &Value) -> String {
    match tab {
        Tab::Notes => format!(
            "{}\n{}\n",
            format_detail_meta_note(v),
            v["content"].as_str().unwrap_or("")
        ),
        Tab::Todos => format!(
            "{}\n\ndone: {}\ndue:  {}\nid:   {}",
            v["title"].as_str().unwrap_or(""),
            v["done"].as_bool().unwrap_or(false),
            v["due_at"].as_str().unwrap_or("—"),
            v["id"].as_str().unwrap_or("")
        ),
        Tab::Reminders => format!(
            "{}\n\nremind_at: {}\nfired:     {}\nid:        {}",
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

fn format_detail_meta_note(v: &Value) -> String {
    let tags = v["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!(
        "tags: {}\nid:   {}\nupd:  {}\n",
        tags,
        v["id"].as_str().unwrap_or(""),
        v["updated_at"].as_str().unwrap_or("")
    )
}

fn format_detail_meta_page(v: &Value) -> String {
    format!(
        "id: {}\nv:  {}\nupd: {}",
        v["id"].as_str().unwrap_or(""),
        v["version"].as_i64().unwrap_or(1),
        v["updated_at"].as_str().unwrap_or("")
    )
}

fn format_ingest_result(v: &Value) -> String {
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

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars.into_iter().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
