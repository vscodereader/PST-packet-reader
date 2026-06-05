//! Publish log batches (알림 — 게시 배치별 결과) domain — JSON-file-backed, served
//! over Tauri IPC. Each batch fans out to a list of per-destination `BatchItem`s.
//! Reuses `PlatformId`/`ModeValue` so the generated bindings stay a single source
//! of truth. Read-only for the UI; `batchStatus`/grouping stay client-side.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::accounts::PlatformId;
use super::posts::ModeValue;
use crate::store::JsonStore;

/// Retain only the most recent N publish batches (mirrors `activity::MAX_ACTIVITY`)
/// so `log-batches.json` can't grow unbounded across months of publishing.
pub const MAX_LOG_BATCHES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum BatchItemStatus {
    Success,
    Fail,
    Running,
    Waiting,
}

/// Single-variant enum → ts-rs emits the `"running"` string-literal type the
/// frontend's optional `state?: "running"` field expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum BatchState {
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub platform: PlatformId,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub board: Option<String>,
    pub login_id: String,
    pub status: BatchItemStatus,
    pub msg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub trace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogBatch {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    #[ts(type = "number")]
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<BatchState>,
    pub items: Vec<BatchItem>,
}

pub fn seed() -> Vec<LogBatch> {
    Vec::new()
}

#[tauri::command]
pub fn list_log_batches(store: tauri::State<'_, JsonStore<LogBatch>>) -> Vec<LogBatch> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_empty() {
        assert!(seed().is_empty());
    }

    #[test]
    fn log_batch_serializes_at_as_number() {
        let b = LogBatch {
            id: "b1".into(),
            title: "테스트".into(),
            kind: ModeValue::Post,
            at: 1_700_000_000_000,
            state: None,
            items: vec![],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"at\":1700000000000"));
        let back: LogBatch = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }
}
