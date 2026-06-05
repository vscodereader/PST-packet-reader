//! 게시글 목록 응답 본문 파싱.
//!
//! 최신글(boardlist)·인기글(WeeklyPopular)은 성공 봉투가 다르므로 파서를
//! 둘로 나누고, 200-with-error / 파싱 불가 폴백은 공유한다. HTTP 전송/상태
//! 코드 처리는 [`super::client`]가 담당한다.
//!
//! # 쿠키 보안
//! 이 모듈은 응답 본문만 다루며 쿠키/세션 값을 절대 포함하지 않는다.

use crate::naver_cafe::article_list::models::{
    ArticleListError, ArticleListResponse, BoardListEnvelope, PopularEnvelope,
};
use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};
use crate::naver_cafe::response::{truncate_body, NaverApiErrorBody};

/// 최신글(boardlist) 2xx 응답 본문을 [`ArticleListResponse`]로 파싱한다.
///
/// 성공 봉투(`{"result":{"articleList":[…]}}`) → `Ok`, 그 외는 [`error_fallback`].
pub fn parse_latest_body(
    status_code: u16,
    raw_body: String,
) -> Result<ArticleListResponse, ArticleListError> {
    if let Ok(envelope) = serde_json::from_str::<BoardListEnvelope>(&raw_body) {
        return Ok(envelope.into_response());
    }
    Err(error_fallback(status_code, raw_body))
}

/// 인기글(WeeklyPopular) 2xx 응답 본문을 [`ArticleListResponse`]로 파싱한다.
///
/// 성공 봉투(`{"message":{"result":{"articleList":[…]}}}`) → `Ok`, 그 외는 [`error_fallback`].
pub fn parse_popular_body(
    status_code: u16,
    raw_body: String,
) -> Result<ArticleListResponse, ArticleListError> {
    if let Ok(envelope) = serde_json::from_str::<PopularEnvelope>(&raw_body) {
        return Ok(envelope.into_response());
    }
    Err(error_fallback(status_code, raw_body))
}

/// 성공 파싱 실패 시 오류 봉투를 만든다: `{"error":{…}}`(200-with-error)면
/// `ARTICLE_LIST_API_ERROR`, 그 외엔 `ARTICLE_LIST_PARSE_ERROR`.
fn error_fallback(status_code: u16, raw_body: String) -> ArticleListError {
    if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
        let trace_id = error_body
            .error
            .more
            .as_ref()
            .and_then(|m| m.request_id.clone())
            .unwrap_or_default();
        tracing::warn!(api_error_code = %error_body.error.error_code, "게시글 목록 조회 API 오류");
        return ErrorEnvelope {
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
        };
    }

    tracing::warn!(status = status_code, "게시글 목록 응답 파싱 실패");
    ErrorEnvelope {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST_FIXTURE: &str = include_str!("fixtures/article_list_latest_success.json");
    const POPULAR_FIXTURE: &str = include_str!("fixtures/article_list_popular_success.json");

    #[test]
    fn parses_latest_success_envelope() {
        let response =
            parse_latest_body(200, LATEST_FIXTURE.to_string()).expect("최신글 성공해야 함");
        assert_eq!(response.articles.len(), 2);
        assert_eq!(response.articles[0].article_id, 12);
    }

    #[test]
    fn parses_popular_success_envelope() {
        let response =
            parse_popular_body(200, POPULAR_FIXTURE.to_string()).expect("인기글 성공해야 함");
        assert_eq!(response.articles.len(), 2);
        assert_eq!(response.articles[0].article_id, 3075152);
    }

    #[test]
    fn wrong_envelope_is_parse_error() {
        // 인기글 본문을 최신글 파서로(또는 반대로) 넣으면 파싱 실패로 떨어진다.
        let err = parse_latest_body(200, POPULAR_FIXTURE.to_string())
            .expect_err("봉투가 다르면 Err여야 함");
        assert_eq!(err.code, "ARTICLE_LIST_PARSE_ERROR");
    }

    #[test]
    fn maps_200_with_error_body_to_api_error() {
        let body = r#"{"error":{"errorCode":"9999","message":"오류가 발생하였습니다."}}"#;
        let err =
            parse_latest_body(200, body.to_string()).expect_err("200-with-error는 Err여야 함");
        assert_eq!(err.code, "ARTICLE_LIST_API_ERROR");
        let data = err.error_data.expect("error_data 없음");
        assert_eq!(data.api_error_code.as_deref(), Some("9999"));
        assert_eq!(data.http_status, Some(200));
    }

    #[test]
    fn maps_unparseable_body_to_parse_error() {
        let err = parse_popular_body(200, "<html>not json</html>".to_string())
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
