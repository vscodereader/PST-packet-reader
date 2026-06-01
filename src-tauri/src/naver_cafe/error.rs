use serde::{Deserialize, Serialize};

use super::models::CafeTarget;

/// 공통 오류 봉투 — 모든 네이버 카페 오류 응답을 감싼다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope<TErrorData> {
    /// 요청 추적 ID.
    pub trace_id: String,
    /// 오류 코드 (예: "POST_FAILED", "VALIDATION_ERROR").
    pub code: String,
    /// 사람이 읽을 수 있는 오류 메시지.
    pub message: String,
    /// 상세 오류 데이터 (선택).
    pub error_data: Option<TErrorData>,
}

/// HTTP / API 수준의 공통 오류 상세 정보.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverCafeCommonErrorData {
    /// 오류가 발생한 카페 대상 (선택).
    pub target: Option<CafeTarget>,
    /// HTTP 응답 상태 코드 (선택).
    pub http_status: Option<u16>,
    /// 네이버 API 오류 코드 (선택).
    pub api_error_code: Option<String>,
    /// 네이버 API 오류 메시지 (선택).
    pub api_error_message: Option<String>,
    /// 재시도 가능 여부.
    pub retryable: bool,
}

/// 요청 필드 유효성 검사 오류.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    /// 유효성 검사에 실패한 필드 이름.
    pub field: String,
    /// 실패 이유 메시지.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target() -> CafeTarget {
        CafeTarget {
            cafe_id: "777".to_string(),
            cafe_name: None,
            menu_id: Some(1),
            article_id: None,
            ref_comment_id: None,
        }
    }

    fn sample_common_error() -> NaverCafeCommonErrorData {
        NaverCafeCommonErrorData {
            target: Some(sample_target()),
            http_status: Some(400),
            api_error_code: Some("ERR_001".to_string()),
            api_error_message: Some("잘못된 요청".to_string()),
            retryable: false,
        }
    }

    #[test]
    fn error_envelope_serializes_camel_case_keys() {
        let envelope: ErrorEnvelope<String> = ErrorEnvelope {
            trace_id: "t-1".to_string(),
            code: "SOME_ERROR".to_string(),
            message: "오류 발생".to_string(),
            error_data: Some("상세".to_string()),
        };
        let json = serde_json::to_value(&envelope).expect("직렬화 실패");

        assert!(json.get("traceId").is_some(), "traceId 키가 없음");
        assert!(json.get("errorData").is_some(), "errorData 키가 없음");
        assert!(json.get("trace_id").is_none(), "snake_case 키가 있으면 안 됨");
    }

    #[test]
    fn error_envelope_round_trips() {
        let original: ErrorEnvelope<String> = ErrorEnvelope {
            trace_id: "t-2".to_string(),
            code: "CODE".to_string(),
            message: "msg".to_string(),
            error_data: None,
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: ErrorEnvelope<String> = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn common_error_data_serializes_camel_case() {
        let data = sample_common_error();
        let json = serde_json::to_value(&data).expect("직렬화 실패");

        assert!(json.get("httpStatus").is_some(), "httpStatus 키가 없음");
        assert!(json.get("apiErrorCode").is_some(), "apiErrorCode 키가 없음");
        assert!(json.get("apiErrorMessage").is_some(), "apiErrorMessage 키가 없음");
        assert!(json.get("retryable").is_some(), "retryable 키가 없음");
    }

    #[test]
    fn common_error_data_round_trips() {
        let original = sample_common_error();
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: NaverCafeCommonErrorData =
            serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn validation_error_round_trips() {
        let original = ValidationError {
            field: "subject".to_string(),
            message: "제목은 필수입니다".to_string(),
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: ValidationError = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }
}
