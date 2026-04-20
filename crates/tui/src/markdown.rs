//! Markdown → ratatui Text rendering.
//!
//! Простой линейный парсер через pulldown-cmark. Поддерживает:
//! - H1-H6 заголовки (разные цвета/жирность)
//! - bold / italic / code inline
//! - list items (`• `, `  ◦ `)
//! - code blocks (выделенный фон)
//! - blockquotes (accent-полоса слева)
//! - horizontal rules
//! - links (подчёркнутые, в цвете link)

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme;

pub fn render(src: &str) -> Text<'static> {
    let parser = Parser::new_ext(src, Options::all());
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut list_depth: usize = 0;
    let mut in_code_block = false;
    let mut in_blockquote = false;

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
                    let _ = dest_url; // displayed as wiki-link style, url suffixed at End
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Heading(_) => {
                    style_stack.pop();
                    flush_line(&mut current, &mut lines);
                    lines.push(Line::from(""));
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link => {
                    style_stack.pop();
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
                    // Code blocks may contain newlines — split into separate lines
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

    Text::from(lines)
}
