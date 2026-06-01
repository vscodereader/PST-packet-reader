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

use crate::auth::read_account_cookies;
use crate::naver_cafe::post::cookie_header_from_storage_state;
use crate::naver_cafe::{
    CafeOrchestrator, ErrorEnvelope, Menu, NaverCafeCommonErrorData, CODE_NO_COOKIES,
};
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

/// Error type for cafe resolution — the same envelope the discovery layer uses,
/// so its errors propagate with `?`.
type ResolveCafeError = ErrorEnvelope<NaverCafeCommonErrorData>;

/// Map the orchestrator's writable [`Menu`] list into cached [`Board`]s.
///
/// `list_boards` already filters to general writable boards, so each menu maps
/// to a board verbatim.
fn boards_from_menus(menus: &[Menu]) -> Vec<Board> {
    menus
        .iter()
        .map(|m| Board {
            name: m.menu_name.clone(),
            menu_id: m.menu_id,
            board_type: m.board_type.clone(),
        })
        .collect()
}

/// Assemble a registrable [`Cafe`] from the resolved id, name, and boards.
fn assemble_cafe(cafe_ref: String, cafe_id: u64, cafe_name: String, menus: &[Menu]) -> Cafe {
    Cafe {
        name: cafe_name,
        cafe_ref,
        cafe_id,
        boards: boards_from_menus(menus),
    }
}

/// Build a NO_COOKIES error envelope. Cookie values are never included.
fn no_cookies_error(account_id: &str, detail: Option<String>) -> ResolveCafeError {
    let message = match detail {
        Some(d) => format!("계정 '{}'의 쿠키를 읽지 못했습니다: {}", account_id, d),
        None => format!(
            "계정 '{}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요.",
            account_id
        ),
    };
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_NO_COOKIES.to_string(),
        message,
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: None,
            api_error_code: None,
            api_error_message: None,
            retryable: false,
        }),
    }
}

/// Resolve a user's cafe reference (URL/slug/numeric) into a registrable cafe.
///
/// Runs the discovery trio once — id resolution, basic info, writable boards —
/// using `account_id`'s stored session cookie. This backs the "+ 카페 추가" flow:
/// the UI caches the result so publishing never re-discovers. Cookie values
/// never appear in the returned error.
#[tauri::command]
pub async fn resolve_cafe(input: String, account_id: String) -> Result<Cafe, ResolveCafeError> {
    let cookie_value = match read_account_cookies(&account_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(no_cookies_error(&account_id, None)),
        Err(e) => return Err(no_cookies_error(&account_id, Some(e.to_string()))),
    };
    let cookie_header = cookie_header_from_storage_state(&cookie_value)
        .ok_or_else(|| no_cookies_error(&account_id, None))?;
    let cookie = Some(cookie_header.as_str());

    let orchestrator = CafeOrchestrator::new();
    let cafe_id = orchestrator.resolve_cafe_id(&input, cookie).await?;
    let info = orchestrator.fetch_cafe_info(cafe_id, cookie).await?;
    let menus = orchestrator.list_boards(cafe_id, cookie).await?;

    Ok(assemble_cafe(input, cafe_id, info.cafe_name, &menus))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::Menu;
    use serde_json::json;

    fn menu(menu_id: u64, name: &str, board_type: &str) -> Menu {
        Menu {
            cafe_id: 31732304,
            menu_id,
            menu_name: name.into(),
            menu_type: "B".into(),
            board_type: board_type.into(),
            writable: true,
            hidden: false,
            separator_menu_type: false,
            use_head: false,
            order: 0,
        }
    }

    #[test]
    fn boards_from_menus_maps_id_name_and_type() {
        let menus = vec![menu(1, "자유게시판", "L"), menu(5, "공지사항", "M")];
        assert_eq!(
            boards_from_menus(&menus),
            vec![
                Board {
                    name: "자유게시판".into(),
                    menu_id: 1,
                    board_type: "L".into(),
                },
                Board {
                    name: "공지사항".into(),
                    menu_id: 5,
                    board_type: "M".into(),
                },
            ]
        );
    }

    #[test]
    fn assemble_cafe_builds_from_parts() {
        let menus = vec![menu(1, "자유게시판", "L")];
        let cafe = assemble_cafe(
            "cafe.naver.com/testcafe".into(),
            31732304,
            "테스트카페".into(),
            &menus,
        );
        assert_eq!(cafe.name, "테스트카페");
        assert_eq!(cafe.cafe_ref, "cafe.naver.com/testcafe");
        assert_eq!(cafe.cafe_id, 31732304);
        assert_eq!(cafe.boards, boards_from_menus(&menus));
    }

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
