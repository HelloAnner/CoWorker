use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("{0}")]
    Message(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("startup error: {0}")]
    Startup(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("app-server request failed: {0}")]
    AppServer(String),
    #[error("duplicate Coworker SSE participant")]
    DuplicateSseParticipant,
    #[error("missing bridge config: {0}")]
    MissingConfig(PathBuf),
}

impl BridgeError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn startup(message: impl Into<String>) -> Self {
        Self::Startup(message.into())
    }
}

pub type Result<T> = std::result::Result<T, BridgeError>;
