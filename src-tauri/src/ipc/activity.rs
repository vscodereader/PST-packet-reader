//! Activity feed (최근 활동 / 시스템 알림) domain — JSON-file-backed, served over
//! Tauri IPC. Read-only for the UI (dashboard timeline + notifications "system"
//! rows), so the only command is `list_activity`.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

const MAX_ACTIVITY: usize = 500;

fn gen_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("ac-{}-{}", crate::util::now_ms(), n)
}

/// 새 활동을 맨 앞에 추가하고 최신 500건만 유지(영속).
pub fn record(store: &JsonStore<ActivityItem>, ty: ActivityType, text: impl Into<String>) {
    let entry = ActivityItem {
        id: gen_id(),
        r#type: ty,
        text: text.into(),
        at: crate::util::now_ms(),
    };
    store.mutate(|mut items| {
        items.insert(0, entry.clone());
        items.truncate(MAX_ACTIVITY);
        items
    });
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ActivityType {
    Success,
    Error,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub id: String,
    // `type` is a Rust keyword; the raw identifier keeps the JSON/TS field name.
    pub r#type: ActivityType,
    pub text: String,
    #[ts(type = "number")]
    pub at: i64,
}

pub fn seed() -> Vec<ActivityItem> {
    Vec::new()
}

#[tauri::command]
pub fn list_activity(store: tauri::State<'_, JsonStore<ActivityItem>>) -> Vec<ActivityItem> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_serializes_with_camelcase_at_and_lowercase_type() {
        let it = ActivityItem {
            id: "ac1".into(),
            r#type: ActivityType::Success,
            text: "테스트".into(),
            at: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&it).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        assert!(json.contains("\"at\":1700000000000"));
        let back: ActivityItem = serde_json::from_str(&json).unwrap();
        assert_eq!(it, back);
    }

    #[test]
    fn seed_is_empty() {
        assert!(seed().is_empty());
    }

    #[test]
    fn record_prepends_newest_first_and_caps_at_500() {
        let dir = std::env::temp_dir().join("pstmacro_activity_record_test");
        let _ = std::fs::remove_dir_all(&dir);
        let store = JsonStore::<ActivityItem>::load_or_seed(dir.join("a.json"), Vec::new());
        for i in 0..520 {
            record(&store, ActivityType::Info, format!("evt {i}"));
        }
        let items = store.snapshot();
        assert_eq!(items.len(), 500); // capped
        assert_eq!(items[0].text, "evt 519"); // newest first
        let _ = std::fs::remove_dir_all(&dir);
    }
}
