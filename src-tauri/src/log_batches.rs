//! Publish log batches (알림 — 게시 배치별 결과) domain — JSON-file-backed, served
//! over Tauri IPC. Each batch fans out to a list of per-destination `BatchItem`s.
//! Reuses `PlatformId`/`ModeValue` so the generated bindings stay a single source
//! of truth. Read-only for the UI; `batchStatus`/grouping stay client-side.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::PlatformId;
use crate::posts::ModeValue;
use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
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
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum BatchState {
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
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
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogBatch {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    pub time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<BatchState>,
    pub items: Vec<BatchItem>,
}

// --------------------------------------------------------------------------
// Seed builders — keep the verbose fixture readable.
// --------------------------------------------------------------------------

/// A forum (종목토론방) destination, keyed by stock `code`.
fn forum(
    target: &str,
    code: &str,
    login_id: &str,
    status: BatchItemStatus,
    msg: &str,
) -> BatchItem {
    BatchItem {
        platform: PlatformId::Forum,
        target: target.into(),
        code: Some(code.into()),
        board: None,
        login_id: login_id.into(),
        status,
        msg: msg.into(),
        trace: None,
    }
}

/// A naver-cafe destination, keyed by `board`.
fn naver(
    target: &str,
    board: &str,
    login_id: &str,
    status: BatchItemStatus,
    msg: &str,
) -> BatchItem {
    BatchItem {
        platform: PlatformId::Naver,
        target: target.into(),
        code: None,
        board: Some(board.into()),
        login_id: login_id.into(),
        status,
        msg: msg.into(),
        trace: None,
    }
}

/// A band destination (no code, no board).
fn band(target: &str, login_id: &str, status: BatchItemStatus, msg: &str) -> BatchItem {
    BatchItem {
        platform: PlatformId::Band,
        target: target.into(),
        code: None,
        board: None,
        login_id: login_id.into(),
        status,
        msg: msg.into(),
        trace: None,
    }
}

fn with_trace(mut item: BatchItem, trace: &str) -> BatchItem {
    item.trace = Some(trace.into());
    item
}

pub fn seed() -> Vec<LogBatch> {
    use BatchItemStatus::*;
    use ModeValue::*;
    vec![
        LogBatch {
            id: "b0".into(),
            title: "삼성전자 4분기 실적 기대 — 매수 관점 정리".into(),
            kind: Post,
            time: "방금 전".into(),
            state: Some(BatchState::Running),
            items: vec![
                forum("삼성전자", "005930", "invest_king7", Success, "게시 완료"),
                forum("SK하이닉스", "000660", "value_pick", Success, "게시 완료"),
                naver("주식투자연구소 카페", "종목분석", "money_lab", Running, "게시 중…"),
            ],
        },
        LogBatch {
            id: "b1".into(),
            title: "5월 이벤트 결과 발표".into(),
            kind: Post,
            time: "오늘 13:48".into(),
            state: None,
            items: vec![
                forum("삼성전자", "005930", "invest_king7", Success, "게시 완료"),
                naver("주식투자연구소 카페", "종목분석", "money_lab", Success, "게시 완료"),
                band("가치투자모임 BAND", "value_invest", Success, "게시 완료"),
            ],
        },
        LogBatch {
            id: "b2".into(),
            title: "반도체 흐름 코멘트 10종".into(),
            kind: Comment,
            time: "오늘 13:42".into(),
            state: None,
            items: vec![
                forum("SK하이닉스", "000660", "value_pick", Success, "댓글 3건 게시"),
                with_trace(
                    forum("한미반도체", "042700", "day_trader_x", Fail, "로그인 세션 만료"),
                    "NaverAuthError: session expired (HTTP 302 → /login)\n    at AuthClient.ensureSession (auth.js:88:13)\n    at async CommentJob.run (jobs/comment.js:142:5)\n    at async Queue.process (queue/runner.js:51:9)\n  hint: 계정 재로그인 후 자동 재시도됩니다.",
                ),
            ],
        },
        LogBatch {
            id: "b3".into(),
            title: "오늘의 특징주 정리".into(),
            kind: Post,
            time: "오늘 12:15".into(),
            state: None,
            items: vec![naver(
                "개미투자 카페",
                "자유게시판",
                "stock_daily",
                Success,
                "게시 완료",
            )],
        },
        LogBatch {
            id: "b4".into(),
            title: "장중 코멘트 세트".into(),
            kind: Comment,
            time: "오늘 11:30".into(),
            state: None,
            items: vec![
                forum("POSCO홀딩스", "005490", "chart_master", Success, "댓글 5건 게시"),
                forum("LG에너지솔루션", "373220", "chart_master", Success, "댓글 5건 게시"),
            ],
        },
        LogBatch {
            id: "b5".into(),
            title: "관심 종목 코멘트".into(),
            kind: Comment,
            time: "어제 19:02".into(),
            state: None,
            items: vec![
                with_trace(
                    naver("주식투자연구소 카페", "종목분석", "money_lab", Fail, "도배 방지 차단"),
                    "RateLimitError: 작성 간격 제한 (cool-down 300s)\n    at SpamGuard.check (guard.js:30:11)\n    at async CommentJob.run (jobs/comment.js:120:5)\n  hint: 게시 간격을 늘리거나 잠시 후 재시도하세요.",
                ),
                forum("셀트리온", "068270", "hot_trend22", Success, "댓글 게시 완료"),
            ],
        },
        LogBatch {
            id: "b6".into(),
            title: "차트 관점 분석".into(),
            kind: Post,
            time: "어제 20:40".into(),
            state: None,
            items: vec![forum("카카오", "035720", "invest_king7", Success, "게시 완료")],
        },
        LogBatch {
            id: "b7".into(),
            title: "주간 시장 브리핑".into(),
            kind: Post,
            time: "5/27 22:30".into(),
            state: None,
            items: vec![
                naver("개미투자 카페", "정보 공유", "stock_daily", Success, "게시 완료"),
                band("가치투자모임 BAND", "value_invest", Success, "게시 완료"),
            ],
        },
    ]
}

#[tauri::command]
pub fn list_log_batches(store: tauri::State<'_, JsonStore<LogBatch>>) -> Vec<LogBatch> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_eight_batches_with_a_running_one() {
        let batches = seed();
        assert_eq!(batches.len(), 8);
        assert!(batches.iter().any(|b| b.state == Some(BatchState::Running)));
        assert!(batches.iter().any(|b| b
            .items
            .iter()
            .any(|i| i.status == BatchItemStatus::Fail && i.trace.is_some())));
    }

    #[test]
    fn running_batch_serializes_state_and_camelcase_fields() {
        let json = serde_json::to_string(&seed()[0]).unwrap();
        assert!(json.contains("\"state\":\"running\""));
        assert!(json.contains("\"loginId\":\"invest_king7\""));
        // Naver item omits `code`, forum item omits `board`.
        assert!(json.contains("\"board\":\"종목분석\""));
    }

    #[test]
    fn seed_roundtrips_through_json() {
        let batches = seed();
        let back: Vec<LogBatch> =
            serde_json::from_str(&serde_json::to_string(&batches).unwrap()).unwrap();
        assert_eq!(batches, back);
    }
}
