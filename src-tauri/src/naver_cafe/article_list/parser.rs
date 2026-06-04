//! 게시글 목록 응답 본문 파싱.
//!
//! 2xx 응답 본문을 받아 성공 봉투 → 200-with-error → 파싱 불가 순으로 해석한다.
//! HTTP 전송/상태 코드 처리는 [`super::client`]가 담당한다.
//!
//! # 쿠키 보안
//! 이 모듈은 응답 본문만 다루며 쿠키/세션 값을 절대 포함하지 않는다.

use crate::naver_cafe::article_list::models::{
    ArticleListEnvelope, ArticleListError, ArticleListResponse,
};
use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};
use crate::naver_cafe::response::{truncate_body, NaverApiErrorBody};

/// 2xx 응답 본문을 [`ArticleListResponse`]로 파싱한다.
///
/// - 성공 봉투(`{"message":{"result":{...}}}`) → `Ok`
/// - `{"error":{...}}` (200-with-error) → `ARTICLE_LIST_API_ERROR`
/// - 그 외 파싱 불가 → `ARTICLE_LIST_PARSE_ERROR`
///
/// `status_code`는 오류 봉투의 `http_status`에 담는다.
pub fn parse_article_list_body(
    status_code: u16,
    raw_body: String,
) -> Result<ArticleListResponse, ArticleListError> {
    // 성공 봉투 먼저 시도 (content-type 무시: 바디가 JSON이면 충분).
    if let Ok(envelope) = serde_json::from_str::<ArticleListEnvelope>(&raw_body) {
        return Ok(envelope.into_response());
    }

    // 2xx인데 {"error":{...}} 형태 (200-with-error)
    if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
        let trace_id = error_body
            .error
            .more
            .as_ref()
            .and_then(|m| m.request_id.clone())
            .unwrap_or_default();
        tracing::warn!(api_error_code = %error_body.error.error_code, "게시글 목록 조회 API 오류");
        return Err(ErrorEnvelope {
            trace_id,
            code: "ARTICLE_LIST_API_ERROR".to_string(),
            message: "게시글 목록 API가 오류를 반환했습니다.".to_string(),
            error_data: Some(NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status_code),
                api_error_code: Some(error_body.error.error_code),
                api_error_message: Some(error_body.error.message),
                retryable: false,
            }),
        });
    }

    // 파싱 불가 폴백
    tracing::warn!(status = status_code, "게시글 목록 응답 파싱 실패");
    Err(ErrorEnvelope {
        trace_id: String::new(),
        code: "ARTICLE_LIST_PARSE_ERROR".to_string(),
        message: "게시글 목록 응답을 파싱하지 못했습니다.".to_string(),
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: Some(status_code),
            api_error_code: None,
            api_error_message: Some(truncate_body(raw_body)),
            retryable: false,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSUMED_FIXTURE: &str = include_str!("fixtures/article_list_success.assumed.json");

    #[test]
    fn parses_success_envelope() {
        let response =
            parse_article_list_body(200, ASSUMED_FIXTURE.to_string()).expect("성공해야 함");
        assert_eq!(response.articles.len(), 2);
        assert!(response.last_page);
    }

    #[test]
    fn maps_200_with_error_body_to_api_error() {
        let body = r#"{"error":{"errorCode":"40004","message":"카페를 찾을 수 없습니다"}}"#;
        let err = parse_article_list_body(200, body.to_string())
            .expect_err("200-with-error는 Err여야 함");
        assert_eq!(err.code, "ARTICLE_LIST_API_ERROR");
        let data = err.error_data.expect("error_data 없음");
        assert_eq!(data.api_error_code.as_deref(), Some("40004"));
        assert_eq!(data.http_status, Some(200));
    }

    #[test]
    fn maps_unparseable_body_to_parse_error() {
        let err = parse_article_list_body(200, "<html>not json</html>".to_string())
            .expect_err("파싱불가는 Err여야 함");
        assert_eq!(err.code, "ARTICLE_LIST_PARSE_ERROR");
        let data = err.error_data.expect("error_data 없음");
        assert!(data.api_error_code.is_none());
        assert_eq!(
            data.api_error_message.as_deref(),
            Some("<html>not json</html>")
        );
    }
}
