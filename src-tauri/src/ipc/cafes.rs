//! Naver cafe destinations (네이버 카페) domain — JSON-file-backed, served over
//! Tauri IPC. These are the user's connected cafes and their boards, used as
//! publish targets. Read-only for the UI; the only command is `list_cafes`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Cafe {
    pub name: String,
    pub boards: Vec<String>,
}

fn cafe(name: &str, boards: &[&str]) -> Cafe {
    Cafe {
        name: name.into(),
        boards: boards.iter().map(|b| (*b).into()).collect(),
    }
}

pub fn seed() -> Vec<Cafe> {
    vec![
        cafe(
            "주식투자연구소 카페",
            &["종목분석", "자유게시판", "질문/답변"],
        ),
        cafe("개미투자 카페", &["자유게시판", "정보 공유", "종목추천"]),
        cafe("가치투자랩 카페", &["공지사항", "종목토론", "자유게시판"]),
    ]
}

#[tauri::command]
pub fn list_cafes(store: tauri::State<'_, JsonStore<Cafe>>) -> Vec<Cafe> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_three_cafes_each_with_boards() {
        let cafes = seed();
        assert_eq!(cafes.len(), 3);
        assert!(cafes.iter().all(|c| !c.boards.is_empty()));
    }

    #[test]
    fn seed_roundtrips_through_json() {
        let cafes = seed();
        let back: Vec<Cafe> =
            serde_json::from_str(&serde_json::to_string(&cafes).unwrap()).unwrap();
        assert_eq!(cafes, back);
    }
}
