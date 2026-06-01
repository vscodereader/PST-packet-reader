use serde::{Deserialize, Serialize};

/// 네이버 카페 게시글 작성 요청.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PostRequest {
    /// 카페 ID.
    pub cafe_id: String,
    /// 게시판(메뉴) ID.
    pub menu_id: u64,
    /// 선택된 게시판의 `boardType` (예: `"L"`).
    ///
    /// 본문 페이로드에는 포함되지 않으며, 글쓰기 Referer 헤더
    /// (`.../articles/write?boardType={board_type}`)에만 사용된다.
    pub board_type: String,
    /// 게시글 제목.
    pub subject: String,
    /// 게시글 본문 텍스트.
    pub body_text: String,
    /// 태그 목록.
    pub tag_list: Vec<String>,
    /// 공개 여부 (선택).
    pub open: Option<bool>,
    /// 네이버 공개 여부 (선택).
    pub naver_open: Option<bool>,
    /// 외부 공개 여부 (선택).
    pub external_open: Option<bool>,
    /// 댓글 허용 여부 (선택).
    pub enable_comment: Option<bool>,
    /// 스크랩 허용 여부 (선택).
    pub enable_scrap: Option<bool>,
    /// 복사 허용 여부 (선택).
    pub enable_copy: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_request() -> PostRequest {
        PostRequest {
            cafe_id: "12345".to_string(),
            menu_id: 99,
            board_type: "L".to_string(),
            subject: "테스트 제목".to_string(),
            body_text: "본문 내용".to_string(),
            tag_list: vec!["태그1".to_string(), "태그2".to_string()],
            open: Some(true),
            naver_open: Some(false),
            external_open: Some(true),
            enable_comment: Some(true),
            enable_scrap: Some(false),
            enable_copy: Some(true),
        }
    }

    #[test]
    fn post_request_serializes_camel_case_keys() {
        let req = full_request();
        let json = serde_json::to_value(&req).expect("직렬화 실패");

        assert!(json.get("cafeId").is_some(), "cafeId 키가 없음");
        assert!(json.get("menuId").is_some(), "menuId 키가 없음");
        assert!(json.get("bodyText").is_some(), "bodyText 키가 없음");
        assert!(json.get("tagList").is_some(), "tagList 키가 없음");
        assert!(json.get("naverOpen").is_some(), "naverOpen 키가 없음");
        assert!(json.get("externalOpen").is_some(), "externalOpen 키가 없음");
        assert!(json.get("enableComment").is_some(), "enableComment 키가 없음");
        assert!(json.get("enableScrap").is_some(), "enableScrap 키가 없음");
        assert!(json.get("enableCopy").is_some(), "enableCopy 키가 없음");

        // snake_case 키는 없어야 함
        assert!(json.get("cafe_id").is_none(), "snake_case cafe_id가 있으면 안 됨");
        assert!(json.get("body_text").is_none(), "snake_case body_text가 있으면 안 됨");
    }

    #[test]
    fn post_request_round_trips() {
        let original = full_request();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: PostRequest = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn post_request_none_options_round_trip() {
        let req = PostRequest {
            cafe_id: "1".to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
            subject: "제목".to_string(),
            body_text: "본문".to_string(),
            tag_list: vec![],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };
        let json = serde_json::to_string(&req).expect("직렬화 실패");
        let restored: PostRequest = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(req, restored);
    }

    #[test]
    fn post_request_empty_tag_list_round_trips() {
        let req = PostRequest {
            cafe_id: "1".to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
            subject: "제목".to_string(),
            body_text: "본문".to_string(),
            tag_list: vec![],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };
        let json = serde_json::to_value(&req).expect("직렬화 실패");
        let tag_list = json.get("tagList").expect("tagList 키가 없음");
        assert!(tag_list.as_array().unwrap().is_empty());
    }
}
