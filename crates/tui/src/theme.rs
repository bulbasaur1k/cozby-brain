//! Единая палитра и стили — вдохновлено Catppuccin Mocha.
//!
//! Принцип: форма = информация, цвет = настроение/состояние.

#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

// ─── Палитра (Catppuccin Mocha) ──────────────────────────────────────

pub const BASE: Color = Color::Rgb(30, 30, 46);
pub const MANTLE: Color = Color::Rgb(24, 24, 37);
pub const SURFACE0: Color = Color::Rgb(49, 50, 68);
pub const SURFACE1: Color = Color::Rgb(69, 71, 90);
pub const TEXT: Color = Color::Rgb(205, 214, 244);
pub const SUBTEXT: Color = Color::Rgb(166, 173, 200);
pub const OVERLAY: Color = Color::Rgb(108, 112, 134);

pub const MAUVE: Color = Color::Rgb(203, 166, 247); // accent (brand)
pub const BLUE: Color = Color::Rgb(137, 180, 250);  // info / links
pub const GREEN: Color = Color::Rgb(166, 227, 161); // ok / strong
pub const YELLOW: Color = Color::Rgb(249, 226, 175);// pending / medium
pub const PEACH: Color = Color::Rgb(250, 179, 135); // warning
pub const RED: Color = Color::Rgb(243, 139, 168);   // error / overdue
pub const TEAL: Color = Color::Rgb(148, 226, 213);  // wiki / doc
pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236);// special

// ─── Базовые стили ───────────────────────────────────────────────────

pub fn text() -> Style { Style::default().fg(TEXT) }
pub fn subtext() -> Style { Style::default().fg(SUBTEXT) }
pub fn overlay() -> Style { Style::default().fg(OVERLAY) }

pub fn accent() -> Style { Style::default().fg(MAUVE).add_modifier(Modifier::BOLD) }
pub fn info() -> Style { Style::default().fg(BLUE) }
pub fn ok() -> Style { Style::default().fg(GREEN) }
pub fn warn() -> Style { Style::default().fg(YELLOW) }
pub fn error() -> Style { Style::default().fg(RED) }
pub fn link() -> Style { Style::default().fg(TEAL) }

pub fn selected_row() -> Style {
    Style::default()
        .bg(SURFACE1)
        .fg(MAUVE)
        .add_modifier(Modifier::BOLD)
}

pub fn active_tab() -> Style {
    Style::default()
        .fg(MAUVE)
        .add_modifier(Modifier::BOLD)
}

pub fn inactive_tab() -> Style { Style::default().fg(OVERLAY) }

// ─── Блоки ───────────────────────────────────────────────────────────

/// Стандартный блок — rounded-рамка цвета overlay, title цвета accent.
pub fn block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .title_style(accent())
        .title(format!(" {title} "))
}

/// Активный блок (фокус) — граница цвета accent.
pub fn block_focused(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(accent())
        .title_style(accent())
        .title(format!(" {title} "))
}

/// Блок-шапка — без верхней границы, только низ/бока.
pub fn block_header(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(SURFACE1))
        .title_style(accent())
        .title(format!(" {title} "))
}
