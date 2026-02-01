//! Error types for the Telegram bot.

use thiserror::Error;

/// Errors that can occur in the Telegram bot.
#[derive(Debug, Error)]
pub enum TelegramError {
    /// Bot token not provided or invalid.
    #[error("Telegram bot token not set. Set TELEGRAM_BOT_TOKEN environment variable.")]
    NoToken,

    /// Failed to start the bot.
    #[error("Failed to start bot: {0}")]
    BotStartFailed(String),

    /// Webhook registration failed.
    #[error("Failed to register webhook: {0}")]
    WebhookFailed(String),

    /// Ngrok tunnel error.
    #[error("Ngrok error: {0}")]
    NgrokError(String),

    /// Ngrok not installed or not in PATH.
    #[error("ngrok not found. Install from https://ngrok.com/download")]
    NgrokNotFound,

    /// Ngrok auth token not set.
    #[error("NGROK_AUTHTOKEN not set. Get a token from https://dashboard.ngrok.com/")]
    NgrokNoAuthToken,

    /// Session error.
    #[error("Session error: {0}")]
    SessionError(String),

    /// User not connected to any project.
    #[error("Not connected to a project. Use /connect <project> first.")]
    NotConnected,

    /// Project not found.
    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    /// Tmux error.
    #[error("Tmux error: {0}")]
    TmuxError(String),

    /// OpenRouter/summarization error.
    #[error("Summarization error: {0}")]
    SummarizationError(String),

    /// Invalid pairing code.
    #[error("Invalid pairing code")]
    InvalidPairingCode,

    /// Pairing code expired.
    #[error("Pairing code expired")]
    PairingExpired,

    /// Not authorized for this project.
    #[error("Not authorized for this project")]
    NotAuthorized,

    /// HTTP request error.
    #[error("HTTP error: {0}")]
    HttpError(String),

    /// IO error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Result type for Telegram operations.
pub type Result<T> = std::result::Result<T, TelegramError>;

impl From<commander_tmux::TmuxError> for TelegramError {
    fn from(e: commander_tmux::TmuxError) -> Self {
        TelegramError::TmuxError(e.to_string())
    }
}

impl From<reqwest::Error> for TelegramError {
    fn from(e: reqwest::Error) -> Self {
        TelegramError::HttpError(e.to_string())
    }
}
