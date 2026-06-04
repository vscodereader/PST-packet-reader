use serde::{Deserialize, Serialize};

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};

/// 네이버 카페 게시판(메뉴) 항목 — 실측 캡처 기준.
///
/// 실측 캡처된 실제 응답:
/// ```json
/// {"cafeId":31732304,"menuId":1,"menuName":"자유게시판","menuType":"B","boardType":"L",...}
/// ```
/// 알 수 없는 필드는 무시된다(deny_unknown_fields 미사용).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    /// 카페 ID.
    pub cafe_id: u64,
    /// 게시판 ID.
    pub menu_id: u64,
    /// 게시판 이름 (예: "자유게시판").
    pub menu_name: String,
    /// 메뉴 유형 — `"B"`는 일반 게시판.
    pub menu_type: String,
    /// 게시판 레이아웃 유형 (예: `"L"`).
    pub board_type: String,
    /// 글쓰기 권한 여부.
    pub writable: bool,
    /// 숨김 여부.
    pub hidden: bool,
    /// 구분선 메뉴 여부.
    pub separator_menu_type: bool,
    /// 머리말 사용 여부 (선택, 기본값 false).
    #[serde(default)]
    pub use_head: bool,
    /// 정렬 순서 (선택, 기본값 0).
    #[serde(default)]
    pub order: u32,
}

/// 게시판 목록 조회 오류 타입 — 공통 오류 봉투 재사용.
pub type MenuError = ErrorEnvelope<NaverCafeCommonErrorData>;

/// 일반 게시판 목록만 반환한다 — `menuType == "B"` && `writable` && `!hidden` && `!separatorMenuType`.
///
/// UI에서 게시판을 선택할 때 표시할 항목만 필터링한다.
pub fn general_writable_boards(menus: &[Menu]) -> Vec<&Menu> {
    menus
        .iter()
        .filter(|m| m.menu_type == "B" && m.writable && !m.hidden && !m.separator_menu_type)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 실측 캡처된 실제 픽스처 항목
    const REAL_FIXTURE_ENTRY: &str = r#"{"cafeId":31732304,"menuId":1,"menuName":"자유게시판","menuType":"B","boardType":"L","writeLevel":1,"readLevel":1,"useHead":false,"useComment":false,"order":1,"writable":true,"hidden":false,"groupPurchase":false,"personalTrade":false,"marketBoardType":false,"accessRestrict":false,"cafeBookMenuType":false,"separatorMenuType":false,"subscribedNaverMeFeed":false}"#;

    fn real_menu() -> Menu {
        serde_json::from_str(REAL_FIXTURE_ENTRY).expect("실제 픽스처 파싱 실패")
    }

    // ------------------------------------------------------------------
    // Menu 구조체 — 역직렬화 및 라운드트립
    // ------------------------------------------------------------------

    #[test]
    fn menu_deserializes_real_fixture_entry() {
        let menu = real_menu();
        assert_eq!(menu.cafe_id, 31732304);
        assert_eq!(menu.menu_id, 1);
        assert_eq!(menu.menu_name, "자유게시판");
        assert_eq!(menu.menu_type, "B");
        assert_eq!(menu.board_type, "L");
        assert!(menu.writable, "writable이 true여야 함");
        assert!(!menu.hidden, "hidden이 false여야 함");
        assert!(
            !menu.separator_menu_type,
            "separatorMenuType이 false여야 함"
        );
        assert_eq!(menu.order, 1);
    }

    #[test]
    fn menu_round_trips() {
        let original = real_menu();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: Menu = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn menu_ignores_unknown_fields() {
        // 알 수 없는 필드가 있어도 파싱에 실패하지 않아야 함
        let json = json!({
            "cafeId": 999_u64,
            "menuId": 2_u64,
            "menuName": "테스트",
            "menuType": "B",
            "boardType": "L",
            "writable": true,
            "hidden": false,
            "separatorMenuType": false,
            "unknownFieldXyz": "ignored"
        });
        let menu: Menu =
            serde_json::from_value(json).expect("알 수 없는 필드가 있어도 파싱 성공해야 함");
        assert_eq!(menu.menu_id, 2);
    }

    // ------------------------------------------------------------------
    // general_writable_boards 필터 함수
    // ------------------------------------------------------------------

    fn make_menu(menu_type: &str, writable: bool, hidden: bool, separator_menu_type: bool) -> Menu {
        Menu {
            cafe_id: 31732304,
            menu_id: 1,
            menu_name: "테스트".to_string(),
            menu_type: menu_type.to_string(),
            board_type: "L".to_string(),
            writable,
            hidden,
            separator_menu_type,
            use_head: false,
            order: 1,
        }
    }

    #[test]
    fn general_writable_boards_keeps_writable_b_board() {
        let menu = real_menu(); // menuType=B, writable=true, hidden=false, separatorMenuType=false
        let menus = [menu];
        let result = general_writable_boards(&menus);
        assert_eq!(result.len(), 1, "일반 게시판 1개가 반환되어야 함");
        assert_eq!(result[0].menu_id, 1);
    }

    #[test]
    fn general_writable_boards_drops_non_writable() {
        let menu = make_menu("B", false, false, false);
        let menus = [menu];
        let result = general_writable_boards(&menus);
        assert!(result.is_empty(), "비쓰기 게시판은 제외되어야 함");
    }

    #[test]
    fn general_writable_boards_drops_hidden() {
        let menu = make_menu("B", true, true, false);
        let menus = [menu];
        let result = general_writable_boards(&menus);
        assert!(result.is_empty(), "숨김 게시판은 제외되어야 함");
    }

    #[test]
    fn general_writable_boards_drops_separator() {
        let menu = make_menu("B", true, false, true);
        let menus = [menu];
        let result = general_writable_boards(&menus);
        assert!(result.is_empty(), "구분선 메뉴는 제외되어야 함");
    }

    #[test]
    fn general_writable_boards_drops_non_b_menu_type() {
        // menuType이 "B"가 아닌 경우 (예: "G" — 그룹 메뉴)
        let menu = make_menu("G", true, false, false);
        let menus = [menu];
        let result = general_writable_boards(&menus);
        assert!(result.is_empty(), "B가 아닌 menuType은 제외되어야 함");
    }

    #[test]
    fn general_writable_boards_mixed_returns_only_valid() {
        let valid = make_menu("B", true, false, false);
        let non_writable = make_menu("B", false, false, false);
        let hidden = make_menu("B", true, true, false);
        let separator = make_menu("B", true, false, true);
        let non_b = make_menu("G", true, false, false);

        let menus = vec![valid, non_writable, hidden, separator, non_b];
        let result = general_writable_boards(&menus);
        assert_eq!(result.len(), 1, "유효한 게시판 1개만 반환되어야 함");
    }
}
