#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("note not found: {0}")]
    NotFound(String),
    #[error("invalid title: must not be empty")]
    InvalidTitle,
    #[error("invalid title: too long (max 256)")]
    TitleTooLong,
}
