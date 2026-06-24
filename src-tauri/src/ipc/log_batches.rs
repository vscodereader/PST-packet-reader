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
    /// 차단 계정으로 첫 글이 로그인/권한 오류로 실패하면, 같은 계정의 남은 글은 시도하지 않고
    /// 이 상태로 건너뛴다(#267-9). X(실패)와 구분해 "건너뜀"으로 표시한다.
    Skip,
}

/// Single-variant enum → ts-rs emits the `"running"` string-literal type the
/// frontend's optional `state?: "running"` field expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum BatchState {
    Running,
}

/// 한 대상에 실제로 게시된 내용(종목별 토큰 치환 후). 글이 빛삭돼도 무엇을 보냈는지
/// 완료 로그에서 확인할 수 있게 종목별로 보존한다. 댓글/URL은 있을 때만.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PostedContent {
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub url: Option<String>,
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
    /// 종목별 실제 게시 내용(제목/본문/댓글/URL). 게시 성공 시에만 채운다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub posted: Option<PostedContent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogBatch {
    pub id: String,
    pub title: String,
    /// 게시 본문 원문(스냅샷). 본문 없는 게시면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    /// 게시 댓글 원문(스냅샷). 댓글 없는 게시면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
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
            body: None,
            comment: None,
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

    #[test]
    fn log_batch_roundtrips_body_and_comment() {
        let b = LogBatch {
            id: "b2".into(),
            title: "제목".into(),
            body: Some("<p>본문</p>".into()),
            comment: Some("좋네요".into()),
            kind: ModeValue::Both,
            at: 1_700_000_000_000,
            state: None,
            items: vec![],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"body\":\"<p>본문</p>\""));
        assert!(json.contains("\"comment\":\"좋네요\""));
        assert_eq!(b, serde_json::from_str::<LogBatch>(&json).unwrap());
    }

    #[test]
    fn log_batch_omits_none_body_comment() {
        let b = LogBatch {
            id: "b3".into(),
            title: "제목".into(),
            body: None,
            comment: None,
            kind: ModeValue::Post,
            at: 1,
            state: None,
            items: vec![],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("body"));
        assert!(!json.contains("comment"));
    }
}
