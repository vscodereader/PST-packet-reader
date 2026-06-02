use serde::{Deserialize, Serialize};

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData, ValidationError};

/// 댓글/대댓글 작성 오류 상세 데이터.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentErrorData {
    /// 공통 네이버 카페 오류 정보.
    pub cafe: NaverCafeCommonErrorData,
    /// 오류가 발생한 게시글 ID (선택).
    pub article_id: Option<String>,
    /// 대댓글인 경우 부모 댓글 ID (선택). 일반 댓글이면 `None`.
    pub ref_comment_id: Option<String>,
    /// 필드 유효성 검사 오류 목록.
    pub validation_errors: Vec<ValidationError>,
}

/// 댓글/대댓글 작성 오류 — 공통 봉투로 감싼 최종 타입.
pub type CommentError = ErrorEnvelope<CommentErrorData>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::models::CafeTarget;

    fn sample_cafe_target() -> CafeTarget {
        CafeTarget {
            cafe_id: "31732304".to_string(),
            cafe_name: Some("샘플 카페".to_string()),
            menu_id: None,
            article_id: Some("4".to_string()),
            ref_comment_id: Some("62598693".to_string()),
        }
    }

    fn sample_common_error() -> NaverCafeCommonErrorData {
        NaverCafeCommonErrorData {
            target: Some(sample_cafe_target()),
            http_status: Some(403),
            api_error_code: Some("NO_PERMISSION".to_string()),
            api_error_message: Some("권한이 없습니다".to_string()),
            retryable: false,
        }
    }

    fn sample_comment_error() -> CommentError {
        ErrorEnvelope {
            trace_id: "trace-cmt-1".to_string(),
            code: "COMMENT_FAILED".to_string(),
            message: "댓글 작성에 실패했습니다".to_string(),
            error_data: Some(CommentErrorData {
                cafe: sample_common_error(),
                article_id: Some("4".to_string()),
                ref_comment_id: Some("62598693".to_string()),
                validation_errors: vec![ValidationError {
                    field: "content".to_string(),
                    message: "본문은 비워둘 수 없습니다".to_string(),
                }],
            }),
        }
    }

    #[test]
    fn comment_error_serializes_camel_case_top_level_keys() {
        let json = serde_json::to_value(&sample_comment_error()).expect("직렬화 실패");
        assert!(json.get("traceId").is_some(), "traceId 키가 없음");
        assert!(json.get("code").is_some(), "code 키가 없음");
        assert!(json.get("message").is_some(), "message 키가 없음");
        assert!(json.get("errorData").is_some(), "errorData 키가 없음");
        assert!(json.get("trace_id").is_none(), "snake_case trace_id가 있으면 안 됨");
    }

    #[test]
    fn comment_error_nested_keys_are_camel_case() {
        let json = serde_json::to_value(&sample_comment_error()).expect("직렬화 실패");
        let error_data = json.get("errorData").unwrap();
        assert!(error_data.get("articleId").is_some(), "articleId 키가 없음");
        assert!(error_data.get("refCommentId").is_some(), "refCommentId 키가 없음");
        assert!(
            error_data.get("validationErrors").is_some(),
            "validationErrors 키가 없음"
        );
    }

    #[test]
    fn comment_error_round_trips() {
        let original = sample_comment_error();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CommentError = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn comment_error_none_error_data_round_trips() {
        let original: CommentError = ErrorEnvelope {
            trace_id: "t".to_string(),
            code: "C".to_string(),
            message: "m".to_string(),
            error_data: None,
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CommentError = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }
}
