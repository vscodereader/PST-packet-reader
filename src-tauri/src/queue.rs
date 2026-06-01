//! Queue (게시 큐) domain — mock → Tauri IPC. Reuses `PlatformId` and
//! `ModeValue` from the accounts/posts modules so the generated TS bindings
//! stay a single source of truth.
//!
//! NOTE (PoC scope): in-memory store, resets on restart. Read + cancel only;
//! the now/scheduled lists are seeded from the frontend mock.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::PlatformId;
use crate::posts::ModeValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum QueueState {
    Running,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueLocation {
    pub p: PlatformId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
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
#[ts(export, export_to = "../../src/shared/bindings/")]
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

pub fn apply_cancel_scheduled(
    items: Vec<QueueScheduledItem>,
    id: &str,
) -> Vec<QueueScheduledItem> {
    items.into_iter().filter(|i| i.id != id).collect()
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
// Managed state + commands
// --------------------------------------------------------------------------

pub struct QueueStore {
    pub now: Mutex<Vec<QueueNowItem>>,
    pub scheduled: Mutex<Vec<QueueScheduledItem>>,
}

impl Default for QueueStore {
    fn default() -> Self {
        QueueStore {
            now: Mutex::new(seed_now()),
            scheduled: Mutex::new(seed_scheduled()),
        }
    }
}

#[tauri::command]
pub fn list_queue_now(store: tauri::State<'_, QueueStore>) -> Vec<QueueNowItem> {
    store.now.lock().expect("queue store poisoned").clone()
}

#[tauri::command]
pub fn list_queue_scheduled(store: tauri::State<'_, QueueStore>) -> Vec<QueueScheduledItem> {
    store.scheduled.lock().expect("queue store poisoned").clone()
}

#[tauri::command]
pub fn cancel_queue_now(store: tauri::State<'_, QueueStore>, id: String) -> Vec<QueueNowItem> {
    let mut guard = store.now.lock().expect("queue store poisoned");
    let next = apply_cancel_now(guard.clone(), &id);
    *guard = next.clone();
    next
}

#[tauri::command]
pub fn cancel_queue_scheduled(
    store: tauri::State<'_, QueueStore>,
    id: String,
) -> Vec<QueueScheduledItem> {
    let mut guard = store.scheduled.lock().expect("queue store poisoned");
    let next = apply_cancel_scheduled(guard.clone(), &id);
    *guard = next.clone();
    next
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
        let sched = seed_scheduled();
        let back2: Vec<QueueScheduledItem> =
            serde_json::from_str(&serde_json::to_string(&sched).unwrap()).unwrap();
        assert_eq!(sched, back2);
    }

    #[test]
    fn now_item_serializes_camelcase_and_omits_none() {
        let json = serde_json::to_string(&seed_now()[1]).unwrap();
        assert!(json.contains("\"state\":\"waiting\""));
        assert!(!json.contains("batchId"));
        assert!(!json.contains("progress"));
    }

    #[test]
    fn running_item_has_progress_tuple() {
        let json = serde_json::to_string(&seed_now()[0]).unwrap();
        assert!(json.contains("\"progress\":[2,3]"));
        assert!(json.contains("\"batchId\":\"b0\""));
    }
}
