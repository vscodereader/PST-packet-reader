//! Queue (게시 큐) domain — JSON-file-backed, wired over Tauri IPC. Reuses
//! `PlatformId`/`ModeValue` from the accounts/posts modules so the generated TS
//! bindings stay a single source of truth.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::accounts::PlatformId;
use super::posts::ModeValue;
use crate::ipc::activity::{record, ActivityType};
use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum QueueState {
    Running,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueLocation {
    pub p: PlatformId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueNowItem {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    pub state: QueueState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub progress: Option<(u32, u32)>,
    pub locs: Vec<QueueLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueScheduledItem {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    pub when: String,
    pub rel: String,
    pub locs: Vec<QueueLocation>,
}

// --------------------------------------------------------------------------
// Pure logic
// --------------------------------------------------------------------------

pub fn apply_cancel_now(items: Vec<QueueNowItem>, id: &str) -> Vec<QueueNowItem> {
    items.into_iter().filter(|i| i.id != id).collect()
}

pub fn apply_cancel_scheduled(items: Vec<QueueScheduledItem>, id: &str) -> Vec<QueueScheduledItem> {
    items.into_iter().filter(|i| i.id != id).collect()
}

/// Convert a scheduled item into a waiting immediate-queue item (for "즉시 처리").
pub fn to_now_item(s: QueueScheduledItem) -> QueueNowItem {
    QueueNowItem {
        id: s.id,
        title: s.title,
        kind: s.kind,
        state: QueueState::Waiting,
        batch_id: None,
        progress: None,
        locs: s.locs,
    }
}

fn loc(p: PlatformId, name: &str, code: Option<&str>) -> QueueLocation {
    QueueLocation {
        p,
        name: name.into(),
        code: code.map(|c| c.into()),
    }
}

pub fn seed_now() -> Vec<QueueNowItem> {
    vec![
        QueueNowItem {
            id: "q1".into(),
            title: "삼성전자 4분기 실적 기대 — 매수 관점 정리".into(),
            kind: ModeValue::Post,
            state: QueueState::Running,
            batch_id: Some("b0".into()),
            progress: Some((2, 3)),
            locs: vec![
                loc(PlatformId::Forum, "삼성전자", Some("005930")),
                loc(PlatformId::Forum, "SK하이닉스", Some("000660")),
                loc(PlatformId::Naver, "주식투자연구소 카페", None),
            ],
        },
        QueueNowItem {
            id: "q2".into(),
            title: "반도체 흐름 코멘트 10종".into(),
            kind: ModeValue::Comment,
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs: vec![loc(PlatformId::Forum, "한미반도체", Some("042700"))],
        },
        QueueNowItem {
            id: "q3".into(),
            title: "오늘의 특징주 정리 — 장 마감 요약".into(),
            kind: ModeValue::Post,
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs: vec![loc(PlatformId::Naver, "개미투자 카페", None)],
        },
    ]
}

pub fn seed_scheduled() -> Vec<QueueScheduledItem> {
    vec![QueueScheduledItem {
        id: "qs1".into(),
        title: "에코프로 조정 구간 대응 전략".into(),
        kind: ModeValue::Both,
        when: "오늘 18:30".into(),
        rel: "5시간 후".into(),
        locs: vec![loc(PlatformId::Forum, "에코프로", Some("086520"))],
    }]
}

// --------------------------------------------------------------------------
// Commands
// --------------------------------------------------------------------------

#[tauri::command]
pub fn list_queue_now(store: tauri::State<'_, JsonStore<QueueNowItem>>) -> Vec<QueueNowItem> {
    store.snapshot()
}

#[tauri::command]
pub fn list_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
) -> Vec<QueueScheduledItem> {
    store.snapshot()
}

#[tauri::command]
pub fn cancel_queue_now(
    store: tauri::State<'_, JsonStore<QueueNowItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<QueueNowItem> {
    let next = store.mutate(|items| apply_cancel_now(items, &id));
    record(activity.inner(), ActivityType::Info, "진행 작업 취소됨");
    next
}

#[tauri::command]
pub fn cancel_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<QueueScheduledItem> {
    let next = store.mutate(|items| apply_cancel_scheduled(items, &id));
    record(activity.inner(), ActivityType::Info, "예약 취소됨");
    next
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// True if `at` is no earlier than the start of the current minute. Minute
/// precision keeps "now" (the picker default) schedulable despite seconds drift.
pub fn is_future_enough(at: i64, now: i64) -> bool {
    at >= now - now.rem_euclid(60_000)
}

/// Append a new scheduled item (used when a post is scheduled from the publish
/// modal). Rejects a past time — defense-in-depth behind the picker's own guard.
#[tauri::command]
pub fn add_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    item: QueueScheduledItem,
    at: i64,
) -> Result<Vec<QueueScheduledItem>, String> {
    if !is_future_enough(at, now_ms()) {
        return Err("예약 시각이 현재보다 과거입니다".into());
    }
    let title = item.title.clone();
    let next = store.mutate(|mut items| {
        items.push(item);
        items
    });
    record(
        activity.inner(),
        ActivityType::Info,
        format!("예약 추가됨 — {title}"),
    );
    Ok(next)
}

/// Move a scheduled item into the immediate queue ("즉시 처리"): drop it from the
/// scheduled store, append it to the now store, and return the updated now list.
#[tauri::command]
pub fn promote_queue_scheduled(
    now: tauri::State<'_, JsonStore<QueueNowItem>>,
    scheduled: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<QueueNowItem> {
    let found = scheduled.snapshot().into_iter().find(|s| s.id == id);
    match found {
        Some(s) => {
            scheduled.mutate(|items| apply_cancel_scheduled(items, &id));
            let next = now.mutate(|mut items| {
                items.push(to_now_item(s));
                items
            });
            record(
                activity.inner(),
                ActivityType::Info,
                "예약을 즉시 게시로 전환",
            );
            next
        }
        None => now.snapshot(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_now_removes_by_id() {
        let next = apply_cancel_now(seed_now(), "q1");
        assert!(next.iter().all(|i| i.id != "q1"));
    }

    #[test]
    fn promote_maps_scheduled_to_a_waiting_now_item() {
        let s = seed_scheduled().remove(0);
        let id = s.id.clone();
        let (title, locs_len) = (s.title.clone(), s.locs.len());
        let now = to_now_item(s);
        assert_eq!(now.id, id);
        assert_eq!(now.title, title);
        assert_eq!(now.state, QueueState::Waiting);
        assert!(now.batch_id.is_none() && now.progress.is_none());
        assert_eq!(now.locs.len(), locs_len);
    }

    #[test]
    fn future_guard_uses_minute_precision() {
        let now: i64 = 1_700_000_045_000; // ...:45s within a minute
        let minute_start = now - now.rem_euclid(60_000);
        assert!(is_future_enough(minute_start, now)); // current minute allowed
        assert!(is_future_enough(now + 60_000, now)); // future allowed
        assert!(!is_future_enough(minute_start - 1, now)); // earlier rejected
    }

    #[test]
    fn cancel_scheduled_removes_by_id() {
        let next = apply_cancel_scheduled(seed_scheduled(), "qs1");
        assert!(next.is_empty());
    }

    #[test]
    fn seeds_roundtrip_through_json() {
        let now = seed_now();
        let back: Vec<QueueNowItem> =
            serde_json::from_str(&serde_json::to_string(&now).unwrap()).unwrap();
        assert_eq!(now, back);
    }

    #[test]
    fn running_item_has_progress_tuple_and_camelcase() {
        let json = serde_json::to_string(&seed_now()[0]).unwrap();
        assert!(json.contains("\"progress\":[2,3]"));
        assert!(json.contains("\"batchId\":\"b0\""));
        assert!(json.contains("\"state\":\"running\""));
    }
}
