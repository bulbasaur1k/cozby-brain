//! Приоритизированное сжатие текста под бюджет слабой LLM (qwen3 с маленьким
//! окном). Не тупая обрезка: нормализует строки (collapse whitespace, dedupe),
//! ранжирует по важности и выбирает топ-K в пределах бюджета.
//!
//! Портировано из cozby-claw `runtime/src/summary_compression.rs`. Главная
//! адаптация под cozby-brain: строки со **ссылками (URL) имеют наивысший
//! приоритет** — для дежурного важно не потерять дашборды/рантбук-ссылки при
//! сжатии tool-результатов и контекста.

use std::collections::BTreeSet;

const DEFAULT_MAX_CHARS: usize = 1_200;
const DEFAULT_MAX_LINES: usize = 24;
const DEFAULT_MAX_LINE_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryCompressionBudget {
    pub max_chars: usize,
    pub max_lines: usize,
    pub max_line_chars: usize,
}

impl Default for SummaryCompressionBudget {
    fn default() -> Self {
        Self {
            max_chars: DEFAULT_MAX_CHARS,
            max_lines: DEFAULT_MAX_LINES,
            max_line_chars: DEFAULT_MAX_LINE_CHARS,
        }
    }
}

impl SummaryCompressionBudget {
    /// Tight budget для одного tool-результата в транскрипте агента.
    pub fn tool_result() -> Self {
        Self {
            max_chars: 400,
            max_lines: 12,
            max_line_chars: 160,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryCompressionResult {
    pub summary: String,
    pub original_chars: usize,
    pub compressed_chars: usize,
    pub original_lines: usize,
    pub compressed_lines: usize,
    pub removed_duplicate_lines: usize,
    pub omitted_lines: usize,
    pub truncated: bool,
}

#[must_use]
pub fn compress_summary(
    summary: &str,
    budget: SummaryCompressionBudget,
) -> SummaryCompressionResult {
    let original_chars = summary.chars().count();
    let original_lines = summary.lines().count();

    let normalized = normalize_lines(summary, budget.max_line_chars);
    if normalized.lines.is_empty() || budget.max_chars == 0 || budget.max_lines == 0 {
        return SummaryCompressionResult {
            summary: String::new(),
            original_chars,
            compressed_chars: 0,
            original_lines,
            compressed_lines: 0,
            removed_duplicate_lines: normalized.removed_duplicate_lines,
            omitted_lines: normalized.lines.len(),
            truncated: original_chars > 0,
        };
    }

    let selected = select_line_indexes(&normalized.lines, budget);
    let mut compressed_lines = selected
        .iter()
        .map(|index| normalized.lines[*index].clone())
        .collect::<Vec<_>>();
    if compressed_lines.is_empty() {
        compressed_lines.push(truncate_line(&normalized.lines[0], budget.max_chars));
    }
    let omitted_lines = normalized.lines.len().saturating_sub(compressed_lines.len());

    if omitted_lines > 0 {
        let omission_notice = omission_notice(omitted_lines);
        push_line_with_budget(&mut compressed_lines, omission_notice, budget);
    }

    let compressed_summary = compressed_lines.join("\n");

    SummaryCompressionResult {
        compressed_chars: compressed_summary.chars().count(),
        compressed_lines: compressed_lines.len(),
        removed_duplicate_lines: normalized.removed_duplicate_lines,
        omitted_lines,
        truncated: compressed_summary != summary.trim(),
        summary: compressed_summary,
        original_chars,
        original_lines,
    }
}

#[must_use]
pub fn compress_summary_text(summary: &str) -> String {
    compress_summary(summary, SummaryCompressionBudget::default()).summary
}

/// Удобный helper: сжать произвольный текст под заданный лимит символов.
#[must_use]
pub fn compress_to_chars(text: &str, max_chars: usize) -> String {
    compress_summary(
        text,
        SummaryCompressionBudget {
            max_chars,
            ..SummaryCompressionBudget::default()
        },
    )
    .summary
}

#[derive(Debug, Default)]
struct NormalizedSummary {
    lines: Vec<String>,
    removed_duplicate_lines: usize,
}

fn normalize_lines(summary: &str, max_line_chars: usize) -> NormalizedSummary {
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    let mut removed_duplicate_lines = 0;

    for raw_line in summary.lines() {
        let normalized = collapse_inline_whitespace(raw_line);
        if normalized.is_empty() {
            continue;
        }

        let truncated = truncate_line(&normalized, max_line_chars);
        let dedupe_key = dedupe_key(&truncated);
        if !seen.insert(dedupe_key) {
            removed_duplicate_lines += 1;
            continue;
        }

        lines.push(truncated);
    }

    NormalizedSummary {
        lines,
        removed_duplicate_lines,
    }
}

fn select_line_indexes(lines: &[String], budget: SummaryCompressionBudget) -> Vec<usize> {
    let mut selected = BTreeSet::<usize>::new();

    for priority in 0..=3 {
        for (index, line) in lines.iter().enumerate() {
            if selected.contains(&index) || line_priority(line) != priority {
                continue;
            }

            let candidate = selected
                .iter()
                .map(|selected_index| lines[*selected_index].as_str())
                .chain(std::iter::once(line.as_str()))
                .collect::<Vec<_>>();

            if candidate.len() > budget.max_lines {
                continue;
            }

            if joined_char_count(&candidate) > budget.max_chars {
                continue;
            }

            selected.insert(index);
        }
    }

    selected.into_iter().collect()
}

fn push_line_with_budget(lines: &mut Vec<String>, line: String, budget: SummaryCompressionBudget) {
    let candidate = lines
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(line.as_str()))
        .collect::<Vec<_>>();

    if candidate.len() <= budget.max_lines && joined_char_count(&candidate) <= budget.max_chars {
        lines.push(line);
    }
}

fn joined_char_count(lines: &[&str]) -> usize {
    lines.iter().map(|line| line.chars().count()).sum::<usize>() + lines.len().saturating_sub(1)
}

/// Приоритет строки (0 = важнейшая, сохраняется первой):
/// 0 — содержит URL (ссылки нельзя терять) или явный core-маркер;
/// 1 — заголовок (markdown `#`/`##` или строка-заголовок с `:`);
/// 2 — пункт списка;
/// 3 — всё остальное.
fn line_priority(line: &str) -> usize {
    if has_url(line) || is_core_detail(line) {
        0
    } else if is_section_header(line) {
        1
    } else if line.starts_with("- ") || line.starts_with("  - ") || line.starts_with("* ") {
        2
    } else {
        3
    }
}

fn has_url(line: &str) -> bool {
    line.contains("http://") || line.contains("https://")
}

fn is_core_detail(line: &str) -> bool {
    ["Summary:", "Conversation summary:", "- Scope:", "- Current work:"]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn is_section_header(line: &str) -> bool {
    line.starts_with('#') || line.ends_with(':')
}

fn omission_notice(omitted_lines: usize) -> String {
    format!("- … {omitted_lines} строк(и) опущено.")
}

fn collapse_inline_whitespace(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    if max_chars == 0 || line.chars().count() <= max_chars {
        return line.to_string();
    }

    if max_chars == 1 {
        return "…".to_string();
    }

    let mut truncated = line
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn dedupe_key(line: &str) -> String {
    line.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_whitespace_and_duplicate_lines() {
        let summary = "Conversation summary:\n\n- Scope:   compact   earlier   messages.\n- Scope: compact earlier messages.\n- Current work: update runtime module.\n";
        let result = compress_summary(summary, SummaryCompressionBudget::default());
        assert_eq!(result.removed_duplicate_lines, 1);
        assert!(result.summary.contains("- Scope: compact earlier messages."));
        assert!(!result.summary.contains("  compact   earlier"));
    }

    #[test]
    fn url_lines_survive_tight_budget() {
        let summary = [
            "Дежурство billing — заметки.",
            "обычный текст без ссылок раз",
            "обычный текст без ссылок два",
            "дашборд https://grafana.example/billing",
            "ещё обычный текст три",
        ]
        .join("\n");
        let result = compress_summary(
            &summary,
            SummaryCompressionBudget {
                max_chars: 80,
                max_lines: 2,
                max_line_chars: 160,
            },
        );
        // URL-строка должна быть отобрана несмотря на жёсткий бюджет.
        assert!(
            result.summary.contains("https://grafana.example/billing"),
            "url line dropped: {}",
            result.summary
        );
        assert!(result.omitted_lines > 0);
    }

    #[test]
    fn provides_a_default_text_only_helper() {
        let summary = "Summary:\n\nA short line.";
        let compressed = compress_summary_text(summary);
        assert_eq!(compressed, "Summary:\nA short line.");
    }
}
