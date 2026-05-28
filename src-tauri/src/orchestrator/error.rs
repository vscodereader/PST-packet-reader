use std::io;

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("wreq: {0}")]
    Http(#[from] wreq::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("runtime path not available: APPDATA is not set")]
    MissingAppData,
    #[error("adb.exe not found after scrcpy extraction")]
    MissingAdb,
    #[error("account not found: {0}")]
    AccountNotFound(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("queue lock poisoned")]
    QueueLock,
}
