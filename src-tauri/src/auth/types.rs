use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub accounts_dir: PathBuf,
    pub cookies_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub accounts_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub password: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QueueJobStatus {
    Pending,
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueJob {
    pub account_id: String,
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub use_adb: bool,
    pub status: QueueJobStatus,
    pub message: String,
    pub queued_at: u128,
    pub started_at: Option<u128>,
    pub finished_at: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    pub is_running: bool,
    pub current_account_id: Option<String>,
    pub jobs: Vec<QueueJob>,
    pub logs: Vec<String>,
}
