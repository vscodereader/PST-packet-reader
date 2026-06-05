use std::io;

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("runtime path not available: LOCALAPPDATA is not set")]
    MissingLocalAppData,
    #[error("account not found: {0}")]
    AccountNotFound(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("queue lock poisoned")]
    QueueLock,
}
