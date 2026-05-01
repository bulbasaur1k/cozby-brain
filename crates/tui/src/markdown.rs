//! Markdown → ratatui Text rendering.
//!
//! Простой линейный парсер через pulldown-cmark. Поддерживает:
//! - H1-H6 заголовки (разные цвета/жирность)
//! - bold / italic / code inline
//! - list items (`• `, `  ◦ `)
//! - code blocks (выделенный фон)
//! - blockquotes (accent-полоса слева)
//! - horizontal rules
//! - links: показываются подчёркнутыми + индексом `[N]`. URL'ы возвращаются
//!   отдельным списком — overlay использует его, чтобы по нажатию 1..9
//!   открывать соответствующую ссылку через `open`/`xdg-open`.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme;

pub struct Rendered {
    pub text: Text<'static>,
    pub links: Vec<String>,
}

pub fn render(src: &str) -> Text<'static> {
    render_with_links(src).text
}

pub fn render_with_links(src: &str) -> Rendered {
    let parser = Parser::new_ext(src, Options::all());
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut list_depth: usize = 0;
    let mut in_code_block = false;
    let mut in_blockquote = false;
    let mut links: Vec<String> = Vec::new();
    // Стек открытых ссылок: индекс в `links` для каждой (вложенность от md
    // редко — но pulldown отдаёт Start/End парами, держим стек на всякий случай).
    let mut link_stack: Vec<usize> = Vec::new();

    let flush_line = |current: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>| {
        if !current.is_empty() {
            lines.push(Line::from(std::mem::take(current)));
        } else {
            lines.push(Line::from(""));
        }
    };

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    if !current.is_empty() {
                        flush_line(&mut current, &mut lines);
                    }
                    let style = match level {
                        HeadingLevel::H1 => theme::accent().add_modifier(Modifier::BOLD),
                        HeadingLevel::H2 => Style::default()
                            .fg(theme::MAUVE)
                            .add_modifier(Modifier::BOLD),
                        HeadingLevel::H3 => theme::info().add_modifier(Modifier::BOLD),
                        _ => theme::warn(),
                    };
                    style_stack.push(style);
                    let prefix = match level {
                        HeadingLevel::H1 => "# ",
                        HeadingLevel::H2 => "## ",
                        HeadingLevel::H3 => "### ",
                        HeadingLevel::H4 => "#### ",
                        HeadingLevel::H5 => "##### ",
                        HeadingLevel::H6 => "###### ",
                    };
                    current.push(Span::styled(prefix.to_string(), style));
                }
                Tag::Emphasis => {
                    let base = *style_stack.last().unwrap_or(&Style::default());
                    style_stack.push(base.add_modifier(Modifier::ITALIC));
                }
                Tag::Strong => {
                    let base = *style_stack.last().unwrap_or(&Style::default());
                    style_stack.push(base.add_modifier(Modifier::BOLD));
                }
                Tag::CodeBlock(_) => {
                    in_code_block = true;
                    if !current.is_empty() {
                        flush_line(&mut current, &mut lines);
                    }
                    style_stack.push(Style::default().fg(theme::PEACH).bg(theme::SURFACE0));
                }
                Tag::List(_) => {
                    list_depth += 1;
                }
                Tag::Item => {
                    if !current.is_empty() {
                        flush_line(&mut current, &mut lines);
                    }
                    let bullet = if list_depth > 1 { "  ◦ " } else { "• " };
                    current.push(Span::styled(
                        bullet.to_string(),
                        Style::default().fg(theme::MAUVE),
                    ));
                }
                Tag::BlockQuote(_) => {
                    in_blockquote = true;
                    if !current.is_empty() {
                        flush_line(&mut current, &mut lines);
                    }
                    current.push(Span::styled(
                        "▌ ".to_string(),
                        Style::default().fg(theme::MAUVE),
                    ));
                    style_stack.push(theme::subtext().add_modifier(Modifier::ITALIC));
                }
                Tag::Paragraph => {}
                Tag::Link { dest_url, .. } => {
                    let base = *style_stack.last().unwrap_or(&Style::default());
                    style_stack.push(
                        base.fg(theme::TEAL).add_modifier(Modifier::UNDERLINED),
                    );
                    links.push(dest_url.to_string());
                    link_stack.push(links.len()); // 1-based индекс
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Heading(_) => {
                    style_stack.pop();
                    flush_line(&mut current, &mut lines);
                    lines.push(Line::from(""));
                }
                TagEnd::Emphasis | TagEnd::Strong => {
                    style_stack.pop();
                }
                TagEnd::Link => {
                    style_stack.pop();
                    if let Some(idx) = link_stack.pop() {
                        // Маркер `[N]` — ярче subtext, чтобы было видно, но
                        // без подчёркивания (ссылочный underline уже снят).
                        current.push(Span::styled(
                            format!(" [{idx}]"),
                            Style::default()
                                .fg(theme::TEAL)
                                .add_modifier(Modifier::DIM),
                        ));
                    }
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    if !current.is_empty() {
                        flush_line(&mut current, &mut lines);
                    }
                    style_stack.pop();
                    lines.push(Line::from(""));
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    if list_depth == 0 {
                        if !current.is_empty() {
                            flush_line(&mut current, &mut lines);
                        }
                        lines.push(Line::from(""));
                    }
                }
                TagEnd::Item => {
                    flush_line(&mut current, &mut lines);
                }
                TagEnd::BlockQuote(_) => {
                    in_blockquote = false;
                    style_stack.pop();
                    flush_line(&mut current, &mut lines);
                    lines.push(Line::from(""));
                }
                TagEnd::Paragraph => {
                    if !current.is_empty() {
                        flush_line(&mut current, &mut lines);
                    }
                    lines.push(Line::from(""));
                }
                _ => {}
            },
            Event::Text(t) => {
                let style = *style_stack.last().unwrap_or(&Style::default());
                if in_code_block {
                    let s = t.to_string();
                    let parts: Vec<&str> = s.split('\n').collect();
                    for (i, part) in parts.iter().enumerate() {
                        if i > 0 {
                            flush_line(&mut current, &mut lines);
                        }
                        if !part.is_empty() {
                            current.push(Span::styled(part.to_string(), style));
                        }
                    }
                } else {
                    current.push(Span::styled(t.to_string(), style));
                }
                let _ = in_blockquote;
            }
            Event::Code(c) => {
                let base = *style_stack.last().unwrap_or(&Style::default());
                current.push(Span::styled(
                    format!("`{c}`"),
                    base.fg(theme::PEACH).bg(theme::SURFACE0),
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut current, &mut lines);
            }
            Event::Rule => {
                if !current.is_empty() {
                    flush_line(&mut current, &mut lines);
                }
                lines.push(Line::from(Span::styled(
                    "─".repeat(60),
                    theme::overlay(),
                )));
                lines.push(Line::from(""));
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        flush_line(&mut current, &mut lines);
    }

    // Сводный список ссылок в конце preview — удобно видеть «куда ведут [N]».
    if !links.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─ links ─".to_string(),
            theme::overlay(),
        )));
        for (i, url) in links.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{}] ", i + 1),
                    theme::subtext().add_modifier(Modifier::BOLD),
                ),
                Span::styled(url.clone(), Style::default().fg(theme::TEAL)),
            ]));
        }
    }

    Rendered {
        text: Text::from(lines),
        links,
    }
}

/// Открывает URL в системном браузере. Ничего не блокирует —
/// fire-and-forget. Возвращает Ok(()) если spawn удался.
pub fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let cmd = "open";

    std::process::Command::new(cmd).arg(url).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_in_order() {
        let src = "see [first](https://a.example) and [second](https://b.example).";
        let r = render_with_links(src);
        assert_eq!(r.links, vec!["https://a.example", "https://b.example"]);
    }

    #[test]
    fn no_links_no_section() {
        let r = render_with_links("plain **text** without links");
        assert!(r.links.is_empty());
    }

    #[test]
    fn link_indices_are_one_based() {
        // На сноске должна появиться `[1]` рядом с текстом.
        let r = render_with_links("[click](https://x)");
        let dump: String = r
            .text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(dump.contains("[1]"));
    }
}
