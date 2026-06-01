//! Scheduled posts (대시보드 게시 대기열) domain — JSON-file-backed, served over
//! Tauri IPC. Distinct from the `queue` domain's scheduled items: this is the
//! dashboard digest keyed to account ids. Reuses `ModeValue` from `posts` so the
//! generated bindings stay a single source of truth. Read-only for the UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::posts::ModeValue;
use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Scheduled {
    pub id: String,
    pub title: String,
    pub accounts: Vec<String>,
    pub kind: ModeValue,
    pub when: String,
    pub rel: String,
}

fn scheduled(
    id: &str,
    title: &str,
    accounts: &[&str],
    kind: ModeValue,
    when: &str,
    rel: &str,
) -> Scheduled {
    Scheduled {
        id: id.into(),
        title: title.into(),
        accounts: accounts.iter().map(|a| (*a).into()).collect(),
        kind,
        when: when.into(),
        rel: rel.into(),
    }
}

pub fn seed() -> Vec<Scheduled> {
    use ModeValue::*;
    vec![
        scheduled(
            "s1",
            "삼성전자 4분기 실적 기대 — 매수 관점 정리",
            &["a1"],
            Post,
            "오늘 14:00",
            "1시간 후",
        ),
        scheduled(
            "s2",
            "오늘 반도체 흐름 좋네요 / 저도 추가 매수했습니다 외",
            &["a2", "a8"],
            Comment,
            "오늘 18:30",
            "5시간 후",
        ),
        scheduled(
            "s3",
            "에코프로 조정 구간 대응 전략",
            &["a3"],
            Both,
            "내일 09:00",
            "내일",
        ),
        scheduled(
            "s4",
            "이번 주 시장 브리핑 정리",
            &["a5", "a6"],
            Post,
            "5/30 12:00",
            "내일",
        ),
        scheduled(
            "s5",
            "POSCO 2차전지 소재 관련 코멘트 모음",
            &["a4"],
            Comment,
            "5/31 20:00",
            "모레",
        ),
    ]
}

#[tauri::command]
pub fn list_scheduled(store: tauri::State<'_, JsonStore<Scheduled>>) -> Vec<Scheduled> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_five_items_with_multi_account_entries() {
        let items = seed();
        assert_eq!(items.len(), 5);
        assert!(items.iter().any(|s| s.accounts.len() == 2));
    }

    #[test]
    fn seed_roundtrips_with_camelcase_kind() {
        let json = serde_json::to_string(&seed()).unwrap();
        assert!(json.contains("\"kind\":\"comment\""));
        let back: Vec<Scheduled> = serde_json::from_str(&json).unwrap();
        assert_eq!(seed(), back);
    }
}
