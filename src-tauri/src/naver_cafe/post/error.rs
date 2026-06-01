use serde::{Deserialize, Serialize};

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData, ValidationError};

/// 게시글 작성 오류 상세 데이터.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PostErrorData {
    /// 공통 네이버 카페 오류 정보.
    pub cafe: NaverCafeCommonErrorData,
    /// 오류가 발생한 메뉴(게시판) ID (선택).
    pub menu_id: Option<u64>,
    /// 오류가 발생한 게시글 제목 (선택).
    pub subject: Option<String>,
    /// 필드 유효성 검사 오류 목록.
    pub validation_errors: Vec<ValidationError>,
}

/// 게시글 작성 오류 — 공통 봉투로 감싼 최종 타입.
pub type PostError = ErrorEnvelope<PostErrorData>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::models::CafeTarget;

    fn sample_cafe_target() -> CafeTarget {
        CafeTarget {
            cafe_id: "999".to_string(),
            cafe_name: Some("샘플 카페".to_string()),
            menu_id: Some(10),
            article_id: None,
            ref_comment_id: None,
        }
    }

    fn sample_common_error() -> NaverCafeCommonErrorData {
        NaverCafeCommonErrorData {
            target: Some(sample_cafe_target()),
            http_status: Some(500),
            api_error_code: Some("INTERNAL".to_string()),
            api_error_message: Some("서버 오류".to_string()),
            retryable: true,
        }
    }

    fn sample_post_error() -> PostError {
        ErrorEnvelope {
            trace_id: "trace-abc-123".to_string(),
            code: "POST_FAILED".to_string(),
            message: "게시글 작성에 실패했습니다".to_string(),
            error_data: Some(PostErrorData {
                cafe: sample_common_error(),
                menu_id: Some(10),
                subject: Some("테스트 제목".to_string()),
                validation_errors: vec![
                    ValidationError {
                        field: "subject".to_string(),
                        message: "제목은 200자 이하여야 합니다".to_string(),
                    },
                    ValidationError {
                        field: "bodyText".to_string(),
                        message: "본문은 비워둘 수 없습니다".to_string(),
                    },
                ],
            }),
        }
    }

    #[test]
    fn post_error_serializes_camel_case_top_level_keys() {
        let err = sample_post_error();
        let json = serde_json::to_value(&err).expect("직렬화 실패");

        assert!(json.get("traceId").is_some(), "traceId 키가 없음");
        assert!(json.get("code").is_some(), "code 키가 없음");
        assert!(json.get("message").is_some(), "message 키가 없음");
        assert!(json.get("errorData").is_some(), "errorData 키가 없음");
        assert!(json.get("trace_id").is_none(), "snake_case trace_id가 있으면 안 됨");
    }

    #[test]
    fn post_error_nested_keys_are_camel_case() {
        let err = sample_post_error();
        let json = serde_json::to_value(&err).expect("직렬화 실패");

        let error_data = json.get("errorData").unwrap();
        assert!(error_data.get("menuId").is_some(), "menuId 키가 없음");
        assert!(error_data.get("validationErrors").is_some(), "validationErrors 키가 없음");

        let cafe = error_data.get("cafe").unwrap();
        assert!(cafe.get("httpStatus").is_some(), "httpStatus 키가 없음");
        assert!(cafe.get("apiErrorCode").is_some(), "apiErrorCode 키가 없음");
        assert!(cafe.get("retryable").is_some(), "retryable 키가 없음");

        let target = cafe.get("target").unwrap();
        assert!(target.get("cafeId").is_some(), "cafeId 키가 없음");
    }

    #[test]
    fn post_error_round_trips() {
        let original = sample_post_error();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: PostError = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn post_error_none_error_data_round_trips() {
        let original: PostError = ErrorEnvelope {
            trace_id: "t".to_string(),
            code: "C".to_string(),
            message: "m".to_string(),
            error_data: None,
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: PostError = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn post_error_data_empty_validation_errors_round_trips() {
        let original: PostError = ErrorEnvelope {
            trace_id: "t".to_string(),
            code: "C".to_string(),
            message: "m".to_string(),
            error_data: Some(PostErrorData {
                cafe: NaverCafeCommonErrorData {
                    target: None,
                    http_status: None,
                    api_error_code: None,
                    api_error_message: None,
                    retryable: false,
                },
                menu_id: None,
                subject: None,
                validation_errors: vec![],
            }),
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: PostError = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);

        // validation_errors가 빈 배열인지 확인
        let val = serde_json::to_value(&original).unwrap();
        let ve = val["errorData"]["validationErrors"].as_array().unwrap();
        assert!(ve.is_empty());
    }
}
