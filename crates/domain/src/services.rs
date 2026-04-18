use chrono::{DateTime, Utc};

use crate::entities::{Note, Reminder, Todo};
use crate::errors::DomainError;

const MAX_TITLE_LEN: usize = 256;

pub fn validate_title(title: &str) -> Result<(), DomainError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(DomainError::InvalidTitle);
    }
    if trimmed.chars().count() > MAX_TITLE_LEN {
        return Err(DomainError::TitleTooLong);
    }
    Ok(())
}

pub fn create_note(
    title: String,
    content: String,
    tags: Vec<String>,
) -> Result<Note, DomainError> {
    validate_title(&title)?;
    Ok(Note::new(title.trim().to_string(), content, normalize_tags(tags)))
}

pub fn update_note(
    existing: Note,
    title: String,
    content: String,
    tags: Vec<String>,
) -> Result<Note, DomainError> {
    validate_title(&title)?;
    Ok(existing.with_update(title.trim().to_string(), content, normalize_tags(tags)))
}

pub fn create_todo(title: String, due_at: Option<DateTime<Utc>>) -> Result<Todo, DomainError> {
    validate_title(&title)?;
    Ok(Todo::new(title.trim().to_string(), due_at))
}

pub fn create_reminder(text: String, remind_at: DateTime<Utc>) -> Result<Reminder, DomainError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(DomainError::InvalidTitle);
    }
    Ok(Reminder::new(trimmed.to_string(), remind_at))
}

/// Извлекает wiki-style ссылки вида `[[target]]` из markdown-контента.
pub fn extract_wiki_links(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = find_closing(content, i + 2) {
                let link = content[i + 2..end].trim();
                if !link.is_empty() {
                    out.push(link.to_string());
                }
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn find_closing(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_empty_title() {
        assert!(validate_title("").is_err());
        assert!(validate_title("   ").is_err());
    }

    #[test]
    fn creates_note_trimmed() {
        let n = create_note("  hello  ".into(), "body".into(), vec![]).unwrap();
        assert_eq!(n.title, "hello");
    }

    #[test]
    fn normalizes_tags() {
        let n = create_note(
            "t".into(),
            "".into(),
            vec!["Rust".into(), "rust".into(), "  ".into(), "Axum".into()],
        )
        .unwrap();
        assert_eq!(n.tags, vec!["axum".to_string(), "rust".to_string()]);
    }

    #[test]
    fn extracts_wiki_links() {
        let links = extract_wiki_links("see [[Note A]] and [[ Note B ]] and plain [text]");
        assert_eq!(links, vec!["Note A".to_string(), "Note B".to_string()]);
    }

    #[test]
    fn update_refreshes_timestamp() {
        let n = create_note("t".into(), "".into(), vec![]).unwrap();
        let created = n.created_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let u = update_note(n, "t2".into(), "x".into(), vec![]).unwrap();
        assert_eq!(u.created_at, created);
        assert!(u.updated_at >= created);
        assert_eq!(u.title, "t2");
    }

    #[test]
    fn todo_completion() {
        let t = create_todo("buy milk".into(), None).unwrap();
        assert!(!t.done);
        let c = t.complete();
        assert!(c.done);
        assert!(c.completed_at.is_some());
    }

    #[test]
    fn reminder_requires_text() {
        assert!(create_reminder("   ".into(), Utc::now()).is_err());
    }
}
