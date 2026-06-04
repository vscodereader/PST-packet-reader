use serde::{Deserialize, Serialize};

use super::models::CafeTarget;
use super::response::{truncate_body, NaverApiErrorBody};

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

/// non-2xx 네이버 카페 응답을 [`NaverCafeCommonErrorData`]를 담은 [`ErrorEnvelope`]로
/// 환원한다. `code`/`message`만 도메인별로 다르고 나머지는 동일하던 로직을 한곳에 모은 것.
///
/// 실측 실패 스키마([`NaverApiErrorBody`]: `{"error":{errorCode,message,more}}`)로
/// 파싱되면 `api_error_code`/`api_error_message`와 `requestId`→`trace_id`를 채우고,
/// 아니면 원본 바디(최대 2000자)를 `api_error_message`에 담는다. `retryable`은
/// `status >= 500`. 쿠키/세션 값은 절대 포함되지 않는다(원본 바디는 truncate만 거친다).
pub fn http_error_envelope(
    status: u16,
    raw_body: String,
    code: &str,
    message: &str,
) -> ErrorEnvelope<NaverCafeCommonErrorData> {
    let retryable = status >= 500;
    if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
        let trace_id = error_body
            .error
            .more
            .as_ref()
            .and_then(|m| m.request_id.clone())
            .unwrap_or_default();
        return ErrorEnvelope {
            trace_id,
            code: code.to_string(),
            message: message.to_string(),
            error_data: Some(NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status),
                api_error_code: Some(error_body.error.error_code),
                api_error_message: Some(error_body.error.message),
                retryable,
            }),
        };
    }
    ErrorEnvelope {
        trace_id: String::new(),
        code: code.to_string(),
        message: message.to_string(),
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: Some(status),
            api_error_code: None,
            api_error_message: Some(truncate_body(raw_body)),
            retryable,
        }),
    }
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
        assert!(
            json.get("trace_id").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
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
        assert!(
            json.get("apiErrorMessage").is_some(),
            "apiErrorMessage 키가 없음"
        );
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

    const REAL_FAILURE_JSON: &str = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}"#;

    #[test]
    fn http_error_envelope_fills_code_message_and_trace_from_real_failure() {
        let env = http_error_envelope(500, REAL_FAILURE_JSON.to_string(), "X_HTTP_ERROR", "실패");
        assert_eq!(env.code, "X_HTTP_ERROR");
        assert_eq!(env.message, "실패");
        assert_eq!(env.trace_id, "cf4ee2db355d4584b6e0add8f8743048");
        let data = env.error_data.expect("error_data 없음");
        assert_eq!(data.http_status, Some(500));
        assert_eq!(data.api_error_code.as_deref(), Some("10404"));
        assert_eq!(data.api_error_message.as_deref(), Some("Page Not Found"));
        assert!(data.retryable, "5xx는 retryable이어야 함");
    }

    #[test]
    fn http_error_envelope_falls_back_to_truncated_body_on_unknown_shape() {
        let env = http_error_envelope(404, "not json".to_string(), "X_HTTP_ERROR", "실패");
        assert_eq!(env.trace_id, "", "파싱 실패 시 trace_id는 비어 있음");
        let data = env.error_data.expect("error_data 없음");
        assert!(data.api_error_code.is_none());
        assert_eq!(data.api_error_message.as_deref(), Some("not json"));
        assert!(!data.retryable, "4xx는 retryable이 아니어야 함");
    }
}
