//! Dashboard stat tiles (운영 계정 / 예약 대기 / …) domain — JSON-file-backed,
//! served over Tauri IPC. Read-only for the UI. The `value` is a number *or* a
//! formatted string (`"97.4%"`), modelled as an untagged enum so ts-rs emits the
//! `number | string` union the frontend already expects.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(untagged)]
pub enum StatValue {
    Num(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DashStat {
    pub key: String,
    pub label: String,
    pub value: StatValue,
    pub sub: String,
    pub icon: String,
    pub color: String,
}

fn stat(key: &str, label: &str, value: StatValue, sub: &str, icon: &str, color: &str) -> DashStat {
    DashStat {
        key: key.into(),
        label: label.into(),
        value,
        sub: sub.into(),
        icon: icon.into(),
        color: color.into(),
    }
}

/// Seed mirrors the snapshot the frontend previously computed from the mock
/// accounts (11 active of 15, 1 error). Once accounts mutate at runtime these
/// can be recomputed server-side; for now they persist as-is.
pub fn seed() -> Vec<DashStat> {
    vec![
        stat(
            "accounts",
            "운영 계정",
            StatValue::Num(11.0),
            "전체 15개 · 오류 1",
            "users",
            "blue",
        ),
        stat(
            "scheduled",
            "예약 대기",
            StatValue::Num(6.0),
            "다음 게시 1시간 후",
            "clock",
            "yellow",
        ),
        stat(
            "today",
            "오늘 게시 완료",
            StatValue::Num(34.0),
            "글 9 · 댓글 25",
            "send",
            "green",
        ),
        stat(
            "rate",
            "게시 성공률",
            StatValue::Text("97.4%".into()),
            "최근 7일",
            "checkCircle",
            "forum",
        ),
    ]
}

#[tauri::command]
pub fn list_stats(store: tauri::State<'_, JsonStore<DashStat>>) -> Vec<DashStat> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_four_tiles_with_expected_keys() {
        let stats = seed();
        let keys: Vec<&str> = stats.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["accounts", "scheduled", "today", "rate"]);
    }

    #[test]
    fn value_serializes_untagged_as_number_or_string() {
        let json = serde_json::to_string(&seed()).unwrap();
        assert!(json.contains("\"value\":11"));
        assert!(json.contains("\"value\":\"97.4%\""));
        let back: Vec<DashStat> = serde_json::from_str(&json).unwrap();
        assert_eq!(seed(), back);
    }
}
