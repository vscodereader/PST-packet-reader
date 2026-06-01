use serde::{Deserialize, Serialize};

/// 네이버 카페 작업 대상 정보 (카페, 메뉴, 게시글 등).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CafeTarget {
    /// 카페 ID (숫자 문자열).
    pub cafe_id: String,
    /// 카페 이름 (선택).
    pub cafe_name: Option<String>,
    /// 게시판(메뉴) ID (선택).
    pub menu_id: Option<u64>,
    /// 게시글 ID (선택).
    pub article_id: Option<String>,
    /// 댓글 참조 ID (선택).
    pub ref_comment_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target() -> CafeTarget {
        CafeTarget {
            cafe_id: "12345".to_string(),
            cafe_name: Some("테스트 카페".to_string()),
            menu_id: Some(99),
            article_id: Some("art-1".to_string()),
            ref_comment_id: None,
        }
    }

    #[test]
    fn cafe_target_serializes_camel_case_keys() {
        let target = sample_target();
        let json = serde_json::to_value(&target).expect("직렬화 실패");

        assert!(json.get("cafeId").is_some(), "cafeId 키가 없음");
        assert!(json.get("cafeName").is_some(), "cafeName 키가 없음");
        assert!(json.get("menuId").is_some(), "menuId 키가 없음");
        assert!(json.get("articleId").is_some(), "articleId 키가 없음");
        assert!(json.get("refCommentId").is_some(), "refCommentId 키가 없음");

        // snake_case 키가 없어야 함
        assert!(json.get("cafe_id").is_none());
    }

    #[test]
    fn cafe_target_round_trips() {
        let original = sample_target();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CafeTarget = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn cafe_target_none_fields_serialize_as_null() {
        let target = CafeTarget {
            cafe_id: "abc".to_string(),
            cafe_name: None,
            menu_id: None,
            article_id: None,
            ref_comment_id: None,
        };
        let json = serde_json::to_string(&target).expect("직렬화 실패");
        let restored: CafeTarget = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(target, restored);
    }
}
