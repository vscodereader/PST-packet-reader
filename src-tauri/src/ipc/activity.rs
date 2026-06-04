//! Activity feed (최근 활동 / 시스템 알림) domain — JSON-file-backed, served over
//! Tauri IPC. Read-only for the UI (dashboard timeline + notifications "system"
//! rows), so the only command is `list_activity`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

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
}
