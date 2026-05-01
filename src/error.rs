//! Error type for terminal operations.

/// Result type used throughout `egaku-term`.
pub type Result<T> = std::result::Result<T, Error>;

/// Failure mode for terminal lifecycle, drawing, or event reading.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Wraps any `std::io::Error` from crossterm or stdout.
    #[error("terminal io: {0}")]
    Io(#[from] std::io::Error),

    /// The application's `update` returned an error. The wrapped value is
    /// whatever the app surfaced.
    #[error("app: {0}")]
    App(String),
}

impl Error {
    /// Construct an [`Error::App`] from any displayable value. Useful when an
    /// app's domain error needs to bubble up out of the runtime.
    pub fn app(msg: impl std::fmt::Display) -> Self {
        Self::App(msg.to_string())
    }
}
