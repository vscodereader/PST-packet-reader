//! Naver cafe destinations (네이버 카페) domain — JSON-file-backed, served over
//! Tauri IPC. These are the user's connected cafes and their boards, used as
//! publish targets.
//!
//! A cafe is registered through the "+ 카페 추가" flow, which resolves the user's
//! URL/slug input into a numeric `cafeId` and discovers its writable boards once,
//! then caches the result here. The publish modal reads this cache directly — it
//! never re-discovers at publish time — so each [`Board`] carries the real
//! `menuId`/`boardType` the article-write API needs.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

/// A single writable board (menu) inside a cafe, as resolved at registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// Display name (e.g. "자유게시판").
    pub name: String,
    /// Numeric board (menu) id — used by the article-write API. Maps to a JS
    /// `number` (not `bigint`): IPC serializes via JSON and cafe/menu ids stay
    /// well within `Number.MAX_SAFE_INTEGER`.
    #[ts(type = "number")]
    pub menu_id: u64,
    /// Board layout type (e.g. "L") — used in the write Referer header.
    pub board_type: String,
}

/// A publish-target cafe. Resolved once at registration and cached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Cafe {
    /// Display name.
    pub name: String,
    /// Original reference the user entered (URL/slug/numeric) — kept for
    /// re-resolution and display.
    pub cafe_ref: String,
    /// Resolved numeric cafe id. Maps to a JS `number` (see [`Board::menu_id`]).
    #[ts(type = "number")]
    pub cafe_id: u64,
    /// Writable boards discovered at registration.
    pub boards: Vec<Board>,
}

/// Cafes are registered by the user (via discovery), so the default seed is empty.
pub fn seed() -> Vec<Cafe> {
    Vec::new()
}

#[tauri::command]
pub fn list_cafes(store: tauri::State<'_, JsonStore<Cafe>>) -> Vec<Cafe> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Cafe {
        Cafe {
            name: "테스트 카페".into(),
            cafe_ref: "cafe.naver.com/testcafe".into(),
            cafe_id: 31732304,
            boards: vec![Board {
                name: "자유게시판".into(),
                menu_id: 1,
                board_type: "L".into(),
            }],
        }
    }

    #[test]
    fn seed_is_empty_until_user_registers() {
        // 카페는 "+ 카페 추가"로 사용자가 직접 해석·등록하므로 기본 시드는 비어 있다.
        assert!(seed().is_empty());
    }

    #[test]
    fn serializes_with_camel_case_keys() {
        let value = serde_json::to_value(sample()).unwrap();
        assert_eq!(
            value,
            json!({
                "name": "테스트 카페",
                "cafeRef": "cafe.naver.com/testcafe",
                "cafeId": 31732304,
                "boards": [
                    { "name": "자유게시판", "menuId": 1, "boardType": "L" }
                ]
            })
        );
    }

    #[test]
    fn roundtrips_through_json() {
        let cafe = sample();
        let back: Cafe =
            serde_json::from_str(&serde_json::to_string(&cafe).unwrap()).unwrap();
        assert_eq!(cafe, back);
    }
}
