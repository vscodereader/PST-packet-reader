use serde::{Deserialize, Serialize};

/// 네이버 카페 일반 댓글 작성 요청.
///
/// `POST apis.naver.com/cafe-web/cafe-mobile/CommentPost.json` 의 입력.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentRequest {
    /// 카페 ID (숫자 문자열, 예: `"31732304"`).
    pub cafe_id: String,
    /// 댓글을 달 게시글 ID.
    pub article_id: String,
    /// 댓글 본문.
    pub content: String,
    /// 스티커 ID (선택). 스티커를 쓰지 않으면 `None`.
    pub sticker_id: Option<String>,
}

/// 네이버 카페 대댓글(답글) 작성 요청.
///
/// `POST apis.naver.com/cafe-web/cafe-mobile/CommentReply.json` 의 입력.
/// 일반 댓글과 달리 부모 댓글 ID([`ref_comment_id`](ReplyRequest::ref_comment_id))를
/// 반드시 포함한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRequest {
    /// 카페 ID (숫자 문자열).
    pub cafe_id: String,
    /// 대댓글을 달 게시글 ID.
    pub article_id: String,
    /// 대댓글 본문.
    pub content: String,
    /// 스티커 ID (선택).
    pub sticker_id: Option<String>,
    /// 답글을 달 **부모 댓글 ID** — 새로 생성될 대댓글 ID가 아니다.
    pub ref_comment_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_comment() -> CommentRequest {
        CommentRequest {
            cafe_id: "31732304".to_string(),
            article_id: "2".to_string(),
            content: "안녕하세요".to_string(),
            sticker_id: None,
        }
    }

    fn sample_reply() -> ReplyRequest {
        ReplyRequest {
            cafe_id: "31732304".to_string(),
            article_id: "4".to_string(),
            content: "답글입니다".to_string(),
            sticker_id: None,
            ref_comment_id: "62598693".to_string(),
        }
    }

    #[test]
    fn comment_request_serializes_camel_case_keys() {
        let json = serde_json::to_value(sample_comment()).expect("직렬화 실패");
        assert!(json.get("cafeId").is_some(), "cafeId 키가 없음");
        assert!(json.get("articleId").is_some(), "articleId 키가 없음");
        assert!(json.get("content").is_some(), "content 키가 없음");
        assert!(json.get("stickerId").is_some(), "stickerId 키가 없음");
        assert!(
            json.get("cafe_id").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
    }

    #[test]
    fn reply_request_includes_ref_comment_id() {
        let json = serde_json::to_value(sample_reply()).expect("직렬화 실패");
        assert!(json.get("refCommentId").is_some(), "refCommentId 키가 없음");
        assert_eq!(json["refCommentId"].as_str().unwrap(), "62598693");
    }

    #[test]
    fn comment_request_round_trips() {
        let original = sample_comment();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CommentRequest = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn reply_request_round_trips() {
        let original = sample_reply();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: ReplyRequest = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn comment_request_sticker_none_round_trips() {
        let mut req = sample_comment();
        req.sticker_id = None;
        let json = serde_json::to_string(&req).expect("직렬화 실패");
        let restored: CommentRequest = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(req, restored);
        assert!(restored.sticker_id.is_none());
    }
}
