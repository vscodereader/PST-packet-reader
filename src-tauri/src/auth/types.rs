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
    Expired,
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
    /// 로컬 쿠키가 유효해 보여도 단락(스킵)하지 않고 실제 재로그인을 강제할지 여부.
    /// 사용자가 명시적으로 선택 계정 로그인을 누른 경우에만 true로 둬, 서버측에서
    /// 죽었지만 로컬 검증만 통과하는 쿠키를 새 값으로 덮어쓴다(이슈 #132).
    #[serde(default)]
    pub force: bool,
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
