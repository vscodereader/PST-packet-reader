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
    pub time: String,
}

fn item(id: &str, ty: ActivityType, text: &str, time: &str) -> ActivityItem {
    ActivityItem {
        id: id.into(),
        r#type: ty,
        text: text.into(),
        time: time.into(),
    }
}

pub fn seed() -> Vec<ActivityItem> {
    use ActivityType::*;
    vec![
        item(
            "ac1",
            Success,
            "‘삼성전자 4분기 실적 기대’ 글이 종목토론방에 게시되었습니다",
            "12분 전",
        ),
        item(
            "ac2",
            Success,
            "반도체 코멘트 10종이 2개 계정에 분산 게시되었습니다",
            "1시간 전",
        ),
        item(
            "ac3",
            Error,
            "한미반도체 토론방 계정 게시 실패 — 로그인 세션 만료",
            "2시간 전",
        ),
        item(
            "ac4",
            Info,
            "종목토론방 12개를 크롤링해 가져왔습니다",
            "3시간 전",
        ),
        item("ac5", Info, "엑셀에서 계정 4건을 가져왔습니다", "어제"),
    ]
}

#[tauri::command]
pub fn list_activity(store: tauri::State<'_, JsonStore<ActivityItem>>) -> Vec<ActivityItem> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_covers_every_activity_type() {
        let items = seed();
        assert_eq!(items.len(), 5);
        assert!(items.iter().any(|i| i.r#type == ActivityType::Success));
        assert!(items.iter().any(|i| i.r#type == ActivityType::Error));
        assert!(items.iter().any(|i| i.r#type == ActivityType::Info));
    }

    #[test]
    fn type_field_serializes_as_lowercase_type() {
        let json = serde_json::to_string(&seed()[0]).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        let back: Vec<ActivityItem> =
            serde_json::from_str(&serde_json::to_string(&seed()).unwrap()).unwrap();
        assert_eq!(seed(), back);
    }
}
